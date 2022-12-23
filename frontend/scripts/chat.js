let ctx = null;
const url = new URL(location.href);
const roomName = url.searchParams.get("room");

if (!roomName) {
    location.href = "/";
}

const ws = new WebSocket(`ws://${location.host}/ws?room=${roomName}`);

document.querySelector("#activate-sound").addEventListener("click", () => {
    ctx = new AudioContext();
});

ws.onmessage = onmessage;
ws.onerror = onerror;
ws.onclose = onclose;



// ===== LISTENING =====



function onmessage(event) {
    console.log("message", event);
    if (event.data.startsWith("dit")) {
        playCorrespondentDit(ctx);
        const [_, name, ...rest] = event.data.split(":");
        addMessage(name, ".");
    } else if (event.data.startsWith("dah")) {
        playCorrespondentDah(ctx);
        const [_, name, ...rest] = event.data.split(":");
        addMessage(name, "_");
    } else if (event.data.startsWith("brk")) {
        const [_, name, ...rest] = event.data.split(":");
        addMessage(name, " ");
    } else if (event.data.startsWith("spc")) {
        const [_, name, ...rest] = event.data.split(":");
        addMessage(name, " (Space) ");
    }
}

function onerror(event) {
    console.log("error", event);
}

function onclose(event) {
    console.log("close", event);
}



// ===== SENDING =====



const CALLSIGN = generateCallsign();
document.querySelector("#callsign").textContent = CALLSIGN;

let osc = null;
const ditlen = 80;
let pressed = false;
const startTimes = {
    press: null,
    pause: null,
};

let spaceSignalAlreadySent = false;
let spaceSignalTimeout = -1;

window.addEventListener("keydown", (event) => {
    if (event.code !== "Space" || pressed) return;
    pressed = true;
    osc = getOscillator(ctx, 440);
    osc?.start();
    startTimes.press = performance.now();

    clearTimeout(spaceSignalTimeout);

    if (startTimes.pause === null) return;
    const pauseDelta = performance.now() - startTimes.pause;
    if (pauseDelta < 3*ditlen) {
        // pause between symbols, do nothing
    } else if (3*ditlen <= pauseDelta && pauseDelta < 7*ditlen) {
        // pause between letters, start new letter
        ws.send("brk:" + CALLSIGN);
        addMessage(CALLSIGN, " ");
    } else {
        // pause between words, insert space
        if (!spaceSignalAlreadySent) {
            ws.send("spc:" + CALLSIGN);
            addMessage(CALLSIGN, " (Space) ");
        }
    }
});


window.addEventListener("keyup", (event) => {
    if (event.code !== "Space") return;
    pressed = false;
    osc?.stop();
    startTimes.pause = performance.now();
    const pressDelta = performance.now() - startTimes.press;
    if (pressDelta < 3*ditlen) {
        ws.send("dit:" + CALLSIGN);
        addMessage(CALLSIGN, ".");
    } else {
        ws.send("dah:" + CALLSIGN);
        addMessage(CALLSIGN, "_");
    }

    spaceSignalAlreadySent = false;
    spaceSignalTimeout = setTimeout(() => {
        ws.send("spc:" + CALLSIGN);
        addMessage(CALLSIGN, " (Space) ");
        spaceSignalAlreadySent = true;
    }, 20*ditlen);
});

function getOscillator(ctx, hz) {
    if (!ctx) return null;
    const osc = ctx.createOscillator();
    osc.type = "sine";
    osc.frequency.setValueAtTime(hz, ctx.currentTime);
    osc.connect(ctx.destination);
    return osc;
}

function generateCallsign(len = 5) {
    let result = "";
    for (let i = 0; i < len; i++) {
        let code = Math.floor(Math.random() * 26) + 0x41;
        result += String.fromCharCode(code);
    }
    return result;
}

function playCorrespondentDit(ctx) {
    const osc = getOscillator(ctx, 880);
    osc?.start();
    setTimeout(() => osc?.stop(), ditlen);
}

function playCorrespondentDah(ctx) {
    const osc = getOscillator(ctx, 880);
    osc?.start();
    setTimeout(() => osc?.stop(), ditlen*3);
}



const messages = {}
messages[CALLSIGN] = [];

function addMessage(user, msg) {
    if (!messages[user])
        messages[user] = [];
    messages[user].push(msg);
    console.log(messages);
    renderMessages();
}

function renderMessages() {
    const parent = document.querySelector("#messages");
    parent.innerHTML = "";
    for (const name in messages) {
        const line = document.createElement("div");
        line.textContent += name;
        line.textContent += ": ";

        let signalsBuffer = [];
        for (const message of messages[name]) {
            if (message === " " || message === " (Space) ") {
                const parsed = signalsToLetter(signalsBuffer);
                signalsBuffer = [];
                line.textContent += " (" + parsed + ")";
            } else if (message !== " (Space) ") {
                signalsBuffer.push(message);
            }
            line.textContent += message;
        }
        parent.appendChild(line);
    }
}

function signalsToLetter(sigs) {
    const signals = sigs.join("");

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

