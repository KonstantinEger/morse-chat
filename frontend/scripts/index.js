async function main() {
  const resp = await fetch("/api/rooms");
  const rooms = await resp.json();
  const roomsContainer = document.querySelector("#rooms-container");
  
  for (const room of rooms) {
    const roomNode = document.createElement("div");
    roomNode.className = "bg-white drop-shadow flex gap-3 rounded w-3/4 md:w-96 mx-auto px-4 py-3";
    roomNode.textContent += room;
    const btn = document.createElement("a");
    btn.className = "ml-auto underline";
    btn.textContent = "join";
    btn.href = `/chat?room=${room}`;
    roomNode.appendChild(btn);
    roomsContainer.appendChild(roomNode);
  }
  
  document.querySelector("#create-room-btn").addEventListener("click", async () => {
    const resp = await fetch("/api/gen-room");
    const json = await resp.json();
    if (json.status === 0) {
      location.href = "/chat?room=" + json.name;
    } else {
      alert("An error occurred. Message: " + json.message);
    }
  });
}

try {
  main()
} catch (e) {
  console.error(e);
}