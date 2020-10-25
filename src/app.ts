import {html, render} from "lit-html";
import {System} from "./System";

console.log("hi~");
render(html`<div>hi~</div>`, document.body);

let system = new System();
console.log("run1:");
system.run();
console.log("run2:");
system.run();