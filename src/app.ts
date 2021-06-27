import {html, render} from "lit-html";
import "./ui/component/CompElement";
import "./ui/PlaygroundElement";
import "./ui/ToolboxElement";
import {System} from "./logic/System";
import {registerBasicComponents} from "./logic/components/basic";
import {Game} from "./game/Game";
import {CELL_SIZE, PLAYGROUND_HEIGHT, PLAYGROUND_WIDTH} from "./util/Constants";

let system = new System();
registerBasicComponents(system);

let game = new Game();

let template1 = {name: "pack4", type: "pack4", w: 6, h: 4};
let template2 = {name: "and", type: "and", w: 4, h: 2};
game.editor.component.createTemplateComponent(template1, 2, 1);
game.editor.component.createTemplateComponent(template2, 12, 1);

let width = CELL_SIZE * PLAYGROUND_WIDTH;
let height = CELL_SIZE * PLAYGROUND_HEIGHT;

render(html`
    <div id="content">
        <style>
            :root {
                --cell-size: ${CELL_SIZE}px;
            }
        </style>

        <div id="container" style="position: absolute; width: ${width}px; height: ${height}px;">
            <playground-element .game=${game}></playground-element>
            <toolbox-element .game=${game}></toolbox-element>
        </div>

        <div style="z-index: 100; position: absolute; left: 0; top: 0;">
            <button @click=${() => game.editor.component.createTemplateComponent(template2, 12, 2)}>createComponent2</button>
        </div>
    </div>
`, document.body);