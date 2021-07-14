import {customElement, html, LitElement, property, PropertyValues} from "lit-element";
import {Game} from "../game/Game";
import {CELL_SIZE, PLAYGROUND_TOP} from "../util/Constants";
import {Events} from "../util/Events";

@customElement('toolbox-element')
export class ToolboxElement extends LitElement {
    @property()
    game: Game;

    @property()
    error: string;

    protected firstUpdated(_changedProperties: PropertyValues) {
        super.firstUpdated(_changedProperties);

        this.game.on(Events.CIRCUIT_RUN, this, () => {
            this.error = null;
            this.requestUpdateInternal();
        }, false, false);
        this.game.on(Events.CIRCUIT_ERROR, this, error => {
            this.error = error;
            this.requestUpdateInternal();
        }, false, false);
    }

    protected render() {
        let height = 4;
        let top = CELL_SIZE * PLAYGROUND_TOP - height / 2;

        let errorDiv = this.error ? html`<div style="color: red;">error: ${this.error}</div>` : html``;

        return html`
            <div class="separation-line" style="z-index: 5; position: absolute; background: white; left: 0; top: ${top}px; width: 100%; height: ${height}px;"></div>
            <div id="toolbox">
<!--                <svg xmlns="http://www.w3.org/2000/svg" class="icon icon-tabler icon-tabler-player-play" width="36" height="36" viewBox="0 0 24 24" stroke-width="1.5" stroke="#2c3e50" fill="none" stroke-linecap="round" stroke-linejoin="round">-->
<!--                    <path stroke="none" d="M0 0h24v24H0z" fill="none"/>-->
<!--                    <path d="M7 4v16l13 -8z" />-->
<!--                </svg>-->
                <button>提交</button>
                ${errorDiv}
            </div>
        `;
    }

    createRenderRoot() {
        return this;
    }

}