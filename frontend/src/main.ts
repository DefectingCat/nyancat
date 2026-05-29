import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { FitAddon } from "@xterm/addon-fit";

const term = new Terminal({
    cursorBlink: true,
    fontSize: 16,
});
const fitAddon = new FitAddon();

term.loadAddon(fitAddon);

const app = document.getElementById("app");
if (!app) {
    throw new Error("Could not find app element");
}

term.open(document.getElementById("app")!);

fitAddon.fit();

const wsUrl = `${window.location.protocol === "https:" ? "wss" : "ws"}://${
    window.location.host
}/ws`;
const ws = new WebSocket(wsUrl);
ws.onopen = () => {
    console.log("connected");
};
ws.onmessage = (ev) => {
    const msg = JSON.parse(ev.data);
    switch (msg.code) {
        case 0:
            const data = {
                code: 1,
                width: term.cols,
                height: term.rows,
            };
            ws.send(JSON.stringify(data));
            break;
        case 1:
            term.write(msg.frame);
            break;
        case 2:
            term.writeln("Error");
            break;
    }
};
ws.onclose = () => {
    console.log("closed");
};
ws.onerror = (ev) => {
    console.log(ev);
};

window.addEventListener("resize", () => {
    fitAddon.fit();

    const data = {
        code: 1,
        width: term.cols,
        height: term.rows,
    };
    if (ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify(data));
    }
});

// term.onKey((ev) => {
//     term.write(ev.key);
//     if (ev.key === "\r") {
//         term.write("\n");
//     }
// });
