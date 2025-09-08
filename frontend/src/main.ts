import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { FitAddon } from "@xterm/addon-fit";
import { ProgressAddon, type IProgressState } from "@xterm/addon-progress";

const term = new Terminal({
    cursorBlink: true,
    fontSize: 16,
});
const fitAddon = new FitAddon();
const progressAddon = new ProgressAddon();

term.loadAddon(fitAddon);
term.loadAddon(progressAddon);

const app = document.getElementById("app");
if (!app) {
    throw new Error("Could not find app element");
}

term.open(document.getElementById("app")!);
// term.write("Hello \\n, World!");

fitAddon.fit();
progressAddon.onChange((state: IProgressState) => {
    console.log(state);
});

const ws = new WebSocket("ws://127.0.0.1:3000/ws");
ws.onopen = () => {
    console.log("connected");
};
ws.onmessage = (ev) => {
    const msg = JSON.parse(ev.data);
    console.log(msg);
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
            term.writeln(msg.frame);
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

// term.onKey((ev) => {
//     term.write(ev.key);
//     if (ev.key === "\r") {
//         term.write("\n");
//     }
// });
