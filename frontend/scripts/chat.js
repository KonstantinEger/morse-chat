const url = new URL(location.href);
const roomName = url.searchParams.get("room");
if (!roomName) {
    location.href = "/";
}
const ws = new WebSocket(`ws://${location.host}/ws?room=${roomName}`);
let ctx = null;
let ownOsc = null;

ws.onmessage = onWebSocketMessage;
ws.onerror = onWebSocketError;

const LETTER_PAUSE_SIGNAL = "letter_pause";
const WORD_PAUSE_SIGNAL = "word_pause";
const DIT_SIGNAL = "dit";
const DAH_SIGNAL = "dah";
const FLUSH_TIME = 1*1000;


const userData = {
    isCurrentlyPressing: false,
    pressStartTime: null,
    pauseStartTime: null,
    ditlen: 100,
    callsign: generateCallsign(),
    currentSignalsBuffer: [],
    flushSignalBufferTimeout: null,
};

document.querySelector("#callsign").textContent = userData.callsign;
document.querySelector("#room-name").textContent = roomName;
{
    const userMsgDiv = document.createElement("div");
    userMsgDiv.id = userData.callsign;
    document.querySelector("#messages").appendChild(userMsgDiv);
}

const peerUserStore = {};

function onWebSocketMessage(event) {
    if (typeof event.data !== "string") return;
    const [message, callsign] = event.data.split(":");
    if (!isValidCallsign(callsign)) {
        console.info(`received message with invalid callsign "${callsign}". ignoring message.`);
        return;
    }
    if (!isValidSignal(message)) {
        console.info(`received invalid signal "${message}". ignoring message.`);
        return;
    }
    if (!document.querySelector("#messages > #" + callsign)) {
        const userMessageElement = document.createElement("div");
        userMessageElement.id = callsign;
        document.querySelector("#messages").appendChild(userMessageElement);
    }
    if (!peerUserStore[callsign]) {
        peerUserStore[callsign] = {
            currentSignalsBuffer: [],
            flushSignalBufferTimeout: null,
        }
    }
    clearTimeout(peerUserStore[callsign].flushSignalBufferTimeout);
    peerUserStore[callsign].currentSignalsBuffer.push(message);
    
    if (message === DIT_SIGNAL) {
        playIncomingDit(ctx, userData.ditlen);
    } else if (message === DAH_SIGNAL) {
        playIncomingDah(ctx, userData.ditlen);
    }
    
    const bubbleHTML = renderMessageBubbleHtml(callsign, peerUserStore[callsign].currentSignalsBuffer);
    document.querySelector("#messages > #" + callsign).innerHTML = bubbleHTML;
    
    peerUserStore[callsign].flushSignalBufferTimeout = setTimeout(() => {
        // todo: render flush
        const bubbleHTML = renderMessageBubbleHtml(callsign, peerUserStore[callsign].currentSignalsBuffer);
        document.querySelector("#old-messages").innerHTML += bubbleHTML;

        document.querySelector("#messages > #" + callsign).innerHTML = "";
        peerUserStore[callsign].currentSignalsBuffer = [];
    }, FLUSH_TIME);
}

function onWebSocketError(error) {
    console.error(error);
    alert("An error occurred. Redirecting to home");
    location.href = "/";
}

globalThis.addEventListener("keydown", event => {
    if (!ctx) {
        ctx = new AudioContext();
    }
    if (event.code !== "Space" || userData.isCurrentlyPressing) return;
    userData.pressStartTime = performance.now();
    userData.isCurrentlyPressing = true;
    
    ownOsc = createOsc(ctx, 440);
    ownOsc?.start();
    
    // abort flushing signals buffer
    clearTimeout(userData.flushSignalBufferTimeout);
    
    if (userData.pauseStartTime === null) return;
    const pauseTime = performance.now() - userData.pauseStartTime;
    if (3*userData.ditlen <= pauseTime && pauseTime < 7*userData.ditlen) {
        // pause between letters
        const signal = LETTER_PAUSE_SIGNAL;
        ws.send(signal + ":" + userData.callsign);
        userData.currentSignalsBuffer.push(signal);
    } else if (7*userData.ditlen <= pauseTime) {
        // pause between words
        const signal = WORD_PAUSE_SIGNAL;
        ws.send(signal + ":" + userData.callsign);
        userData.currentSignalsBuffer.push(signal);
    }
});

globalThis.addEventListener("keyup", event => {
    if (event.code !== "Space") return;
    userData.pauseStartTime = performance.now();
    userData.isCurrentlyPressing = false;
    
    ownOsc?.stop();
    
    if (userData.pressStartTime === null) return;
    const pressTime = performance.now() - userData.pressStartTime;
    let signal;
    if (pressTime < 3 * userData.ditlen) {
        signal = DIT_SIGNAL;
    } else {
        signal = DAH_SIGNAL;
    }
    ws.send(signal + ":" + userData.callsign);
    userData.currentSignalsBuffer.push(signal);
    
    const bubbleHTML = renderMessageBubbleHtml(userData.callsign, userData.currentSignalsBuffer);
    document.querySelector("#messages > #" + userData.callsign).innerHTML = bubbleHTML;
    
    // flush signals buffer
    userData.flushSignalBufferTimeout = setTimeout(flushUserSignalBuffer, FLUSH_TIME);
});

function flushUserSignalBuffer() {
    console.debug("flushing signal data buffer");

    const bubbleHtml = renderMessageBubbleHtml(userData.callsign, userData.currentSignalsBuffer);
    document.querySelector("#old-messages").innerHTML += bubbleHtml;

    userData.pauseStartTime = null;
    userData.pressStartTime = null;
    userData.currentSignalsBuffer = [];
    document.querySelector("#messages > #" + userData.callsign).innerHTML = "";
}

function signalsBufferToHtml(signals) {
    let result = "<div>";
    let currentLetterBuff = [];
    for (const signal of signals) {
        if (signal === DIT_SIGNAL || signal === DAH_SIGNAL) {
            result += ditDahToDotDashUni(signal);
            const dotOrDash = ditDahToDotDashPunct(signal);
            currentLetterBuff.push(dotOrDash);
        } else {
            const letter = morseSignToLetter(currentLetterBuff.join(""));
            result += `<span class="text-gray-300">(${letter})</span>`;
            currentLetterBuff = [];
        }
    }
    const letter = morseSignToLetter(currentLetterBuff.join(""));
    result += `<span class="text-gray-300">(${letter})</span>`;
    result += "</div>"
    return result;
}

function renderMessageBubbleHtml(callsign, signals) {
    let bubble = `<div class="gap-3 flex ${callsign === userData.callsign ? "flex-row-reverse" : "fex-row"}">`;
    bubble += `<div class="bg-white drop-shadow rounded-full flex justify-center items-center p-3">${callsign}</div>`;
    bubble += `<div class="bg-white drop-shadow px-5 py-4 rounded-xl">${signalsBufferToHtml(signals)}</div>`;
    bubble += "</div>";
    return bubble;
}

function ditDahToDotDashPunct(s) {
    if (s === DIT_SIGNAL) {
        return ".";
    } else if (s === DAH_SIGNAL) {
        return "_";
    } else {
        return s;
    }
}

function ditDahToDotDashUni(s) {
    if (s === DIT_SIGNAL) {
        return "&bull;";
    } else if (s === DAH_SIGNAL) {
        return "&mdash;";
    } else {
        return s;
    }
}

function morseSignToLetter(signals) {
    if (signals === "._") return "A"
    else if (signals === "_...") return "B"
    else if (signals === "_._.") return "C"
    else if (signals === "_..") return "D"
    else if (signals === ".") return "E"
    else if (signals === ".._.") return "F"
    else if (signals === "__.") return "G"
    else if (signals === "....") return "H"
    else if (signals === "..") return "I"
    else if (signals === ".___") return "J"
    else if (signals === "_._") return "K"
    else if (signals === "._..") return "L"
    else if (signals === "__") return "M"
    else if (signals === "_.") return "N"
    else if (signals === "___") return "O"
    else if (signals === ".__.") return "P"
    else if (signals === "__._") return "Q"
    else if (signals === "._.") return "R"
    else if (signals === "...") return "S"
    else if (signals === "_") return "T"
    else if (signals === ".._") return "U"
    else if (signals === "..._") return "V"
    else if (signals === ".__") return "W"
    else if (signals === "_.._") return "X"
    else if (signals === "_.__") return "Y"
    else if (signals === "__..") return "Z"
    else if (signals === ".____") return "1"
    else if (signals === "..___") return "2"
    else if (signals === "...__") return "3"
    else if (signals === "...._") return "4"
    else if (signals === ".....") return "5"
    else if (signals === "_....") return "6"
    else if (signals === "__...") return "7"
    else if (signals === "___..") return "8"
    else if (signals === "____.") return "9"
    else if (signals === "_____") return "0"
    else if (signals === "........") return "Sry, Fehler"
    else return "{{?}}";
}

function generateCallsign(len = 5) {
    let result = "";
    for (let i = 0; i < len; i++) {
        const code = Math.floor(Math.random() * 26) + 0x41;
        result += String.fromCharCode(code);
    }
    return result;
}

function isValidCallsign(sign) {
    return sign.length === 5 && isAlphabetic(sign);
}

function isAlphabetic(sign) {
    const signUpper = sign.toUpperCase();
    let result = true;
    for (const c of signUpper) {
        const code = c.charCodeAt(0);
        result = result && 0x41 <= code && code <= 0x5a;
    }
    return result;
}

function isValidSignal(signal) {
    return signal === DIT_SIGNAL
        || signal === DAH_SIGNAL
        || signal === LETTER_PAUSE_SIGNAL
        || signal === WORD_PAUSE_SIGNAL;
}

function createOsc(ctx, hz) {
    if (!ctx) return null;
    const osc = ctx.createOscillator();
    osc.type = "sine";
    osc.frequency.setValueAtTime(hz, ctx.currentTime);
    osc.connect(ctx.destination);
    return osc;
}

function playIncomingDit(ctx, ditlen) {
    if (!ctx) return;
    const osc = createOsc(ctx, 880);
    osc?.start();
    setTimeout(() => osc?.stop(), ditlen);
}

function playIncomingDah(ctx, ditlen) {
    if (!ctx) return;
    const osc = createOsc(ctx, 880);
    osc?.start();
    setTimeout(() => osc?.stop(), 3*ditlen);
}
