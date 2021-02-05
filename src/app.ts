import {html, render} from "lit-html";
import "./ui/component/GameCompElement";
import {System} from "./logic/System";
import {registerBasicComponents} from "./logic/components/basic";
import {GameComp} from "./game/GameComp";

let system = new System();
registerBasicComponents(system);

let gameComp = new GameComp(1, system, {name: "pack4", type: "pack4", w: 6, h: 2, x: 2, y: 2});

render(html`<gamecomp-element .gameComp=${gameComp} style="position: absolute; left: 100px;"></gamecomp-element>`, document.body);