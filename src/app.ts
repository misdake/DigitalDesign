import {html, render} from "lit-html";
import "./ui/component/GameCompElement";
import {System} from "./logic/System";
import {registerBasicComponents} from "./logic/components/basic";
import {GameComp} from "./game/GameComp";

let system = new System();
registerBasicComponents(system);

let gameComp1 = new GameComp(1, system, {name: "pack4", type: "pack4", w: 6, h: 4, x: 2, y: 2});
let gameComp2 = new GameComp(2, system, {name: "and", type: "and", w: 4, h: 2, x: 2, y: 2});

render(html`<gamecomp-element .gameComp=${gameComp1} style="position: absolute; left: 100px;"></gamecomp-element>`, document.body);