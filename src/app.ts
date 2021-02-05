import {html, render} from "lit-html";
import "./ui/component/ComponentElement";
import {System} from "./logic/System";
import {registerBasicComponents} from "./logic/components/basic";

let system = new System();
registerBasicComponents(system);
let component = system.createComponent("and_gate", "and");

console.log("hi~");
render(html`<component-element .component=${component} style="position: absolute; left: 100px;"></component-element>`, document.body);