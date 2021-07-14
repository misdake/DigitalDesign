import {customElement, html, LitElement, property, PropertyValues} from "lit-element";
import {Game} from "../game/Game";
import {CELL_SIZE, PLAYGROUND_TOP} from "../util/Constants";
import {Events} from "../util/Events";

@customElement('toolbox-element')
export class ToolboxElement extends LitElement {
    @property()
    game: Game;

    @property()
    result: string;
    @property()
    error: string;

    protected firstUpdated(_changedProperties: PropertyValues) {
        super.firstUpdated(_changedProperties);

        this.game.on(Events.CIRCUIT_RUN, this, result => {
            this.result = result;
            this.error = null;
            this.requestUpdateInternal();
        }, false, false);
        this.game.on(Events.CIRCUIT_ERROR, this, error => {
            this.result = null;
            this.error = error;
            this.requestUpdateInternal();
        }, false, false);
    }

    private submit() {
        this.game.test();
    }

    protected render() {
        let height = 4;
        let top = CELL_SIZE * PLAYGROUND_TOP - height / 2;

        let resultDiv = this.result ? html`<div style="color: black;">result: ${this.result}</div>` : html``;
        let errorDiv = this.error ? html`<div style="color: red;">error: ${this.error}</div>` : html``;

        return html`
            <div class="separation-line" style="z-index: 5; position: absolute; background: white; left: 0; top: ${top}px; width: 100%; height: ${height}px;"></div>
            <div id="toolbox">
                <button @click=${this.submit}>submit and test</button>
                ${resultDiv}
                ${errorDiv}
            </div>
        `;
    }

    createRenderRoot() {
        return this;
    }

}