import {customElement, html, LitElement, property} from "lit-element";
import {Game} from "../game/Game";
import {CELL_SIZE, PLAYGROUND_TOP} from "../util/Constants";

@customElement('toolbox-element')
export class ToolboxElement extends LitElement {
    @property()
    game: Game;

    protected render() {
        let height = 4;
        let top = CELL_SIZE * PLAYGROUND_TOP - height / 2;

        return html`
            <div class="separation-line" style="z-index: 5; position: absolute; background: white; left: 0; top: ${top}px; width: 100%; height: ${height}px;"></div>
            <div id="toolbox">
<!--                <svg xmlns="http://www.w3.org/2000/svg" class="icon icon-tabler icon-tabler-player-play" width="36" height="36" viewBox="0 0 24 24" stroke-width="1.5" stroke="#2c3e50" fill="none" stroke-linecap="round" stroke-linejoin="round">-->
<!--                    <path stroke="none" d="M0 0h24v24H0z" fill="none"/>-->
<!--                    <path d="M7 4v16l13 -8z" />-->
<!--                </svg>-->
                <button>提交</button>
            </div>
        `;
    }

    createRenderRoot() {
        return this;
    }

}