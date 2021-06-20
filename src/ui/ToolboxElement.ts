import {customElement, html, LitElement, property} from "lit-element";
import {Game} from "../game/Game";

@customElement('toolbox-element')
export class ToolboxElement extends LitElement {
    @property()
    game: Game;

    protected render() {
        return html`
            <div id="inventory"></div>
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