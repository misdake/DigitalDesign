import {html, render} from "lit-html";
import "./ui/component/GameCompElement";
import {System} from "./logic/System";
import {registerBasicComponents} from "./logic/components/basic";
import {GameComp} from "./game/GameComp";
import {Game} from "./game/Game";

let system = new System();
registerBasicComponents(system);

let pack1 = {name: "pack4", type: "pack4", w: 6, h: 4, x: 2, y: 2};
let pack2 = {name: "and", type: "and", w: 4, h: 2, x: 12, y: 2};
let gameComp1 = new GameComp(1, system, pack1);
let gameComp2 = new GameComp(2, system, pack2);

let game = new Game();

render(html`
    <div id="content">
        <gamecomp-element .game=${game} .gameComp=${gameComp1} style="position: absolute;"></gamecomp-element>
        <gamecomp-element .game=${game} .gameComp=${gameComp2} style="position: absolute;"></gamecomp-element>
    </div>
`, document.body);