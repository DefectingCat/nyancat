import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { FitAddon } from "@xterm/addon-fit";

const term = new Terminal({
    cursorBlink: true,
    fontSize: 16,
});
const fitAddon = new FitAddon();
term.loadAddon(fitAddon);

term.open(document.getElementById("app")!);

term.write("Hello, World!");
fitAddon.fit();
