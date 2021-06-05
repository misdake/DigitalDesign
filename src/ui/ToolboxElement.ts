import {customElement, html, LitElement, property} from "lit-element";
import {Game} from "../game/Game";

@customElement('toolbox-element')
export class ToolboxElement extends LitElement {
    @property()
    game: Game;

    protected render() {
        return html`
            <div class="toolbox">
                <div class="inventory"></div>
                <div class="levelcontrol"></div>
            </div>
        `;
    }

    createRenderRoot() {
        return this;
    }

}