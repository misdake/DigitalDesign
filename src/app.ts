import {html, render} from "lit-html";
import "./ui/component/CompElement";
import "./ui/PlaygroundElement";
import "./ui/ToolboxElement";
import {System} from "./logic/System";
import {registerBasicComponents} from "./logic/components/basic";
import {Game} from "./game/Game";
import {CELL_SIZE, PLAYGROUND_HEIGHT, PLAYGROUND_WIDTH} from "./util/Constants";
import {GameCompTemplate} from "./game/GameComp";

let system = new System();
registerBasicComponents(system);

let game = new Game();

let templates = [
    {name: "pack4", type: "pack4", w: 6, h: 4},
    {name: "and", type: "and", w: 4, h: 2},
    {name: "xor", type: "xor", w: 4, h: 2},
];

function addTemplateComponent(template: GameCompTemplate, startX: number) {
    game.editor.component.createTemplateComponent(template, startX, 1);
}

function addTemplateComponents() {
    let x = 1;
    for (let template of templates) {
        addTemplateComponent(template, x);
        x += 3;
    }
}
addTemplateComponents();

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
    </div>
`, document.body);