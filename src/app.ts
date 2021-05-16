import {html, render} from "lit-html";
import "./ui/component/GameCompElement";
import "./ui/PlaygroundElement";
import {System} from "./logic/System";
import {registerBasicComponents} from "./logic/components/basic";
import {Game} from "./game/Game";

let system = new System();
registerBasicComponents(system);

let game = new Game();

let template1 = {name: "pack4", type: "pack4", w: 6, h: 4};
let template2 = {name: "and", type: "and", w: 4, h: 2};
game.editor.component.createComponent(template1, 2, 2);
// this.editor.createComponent(template2, 12, 2);

render(html`
    <div id="content">
        <playground-element .game=${game}></playground-element>

        <div style="position: absolute; left: 0; top: 0;">
            <button @click=${() => game.editor.component.createComponent(template2, 12, 2)}>createComponent2</button>
        </div>
    </div>
`, document.body);