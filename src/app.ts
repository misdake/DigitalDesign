import {html, render} from "lit-html";
import {EmulatorSystem} from "./System";

console.log("hi~");

render(html`<div>hi~</div>`, document.body);

let system = new EmulatorSystem();
system.loadGraph();
system.run();