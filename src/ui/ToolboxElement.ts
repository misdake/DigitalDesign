import {customElement, html, LitElement, property} from "lit-element";
import {Game} from "../game/Game";
import {CELL_SIZE} from "../util/Constants";

@customElement('toolbox-element')
export class ToolboxElement extends LitElement {
    @property()
    game: Game;

    protected render() {
        let height = 4;
        let top = CELL_SIZE * 3 - height / 2;

        return html`
            <div id="inventory" style="z-index: 30; position: absolute; background: white; left: 0; top: ${top}px; width: 100%; height: ${height}px;"></div>
            <div id="toolbox">
                <button>提交</button>
                <button>debug类按钮</button>
            </div>
        `;
    }

    createRenderRoot() {
        return this;
    }

}