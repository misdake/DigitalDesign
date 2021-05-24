import {customElement, html, LitElement, property} from "lit-element";
import {Game} from "../../game/Game";
import {GameWire} from "../../game/GameWire";

@customElement('wire-element')
export class WireElement extends LitElement {
    @property()
    game: Game;
    @property()
    gameWire: GameWire;

    protected render() {
        let {x: x1, y: y1} = this.gameWire.fromPin.getXy();
        let {x: x2, y: y2} = this.gameWire.toPin.getXy();

        x1 = x1 * 50 + 25;
        x2 = x2 * 50 + 25;
        y1 = y1 * 50 + 25;
        y2 = y2 * 50 + 25;

        let minX = Math.min(x1, x2) - 10;
        let minY = Math.min(y1, y2) - 10;
        let maxX = Math.max(x1, x2) + 10;
        let maxY = Math.max(y1, y2) + 10;

        return html`
            <svg class="pin-svg" width="${(maxX - minX)}" height="${(maxY - minY)}" style="position: absolute; left: ${minX}px; top: ${minY}px; pointer-events: none;">
                <line class="pin-wire" stroke="red" stroke-width="5px" stroke-linecap="round"
                      x1=${(x1 - minX)}
                      y1=${(y1 - minY)}
                      x2=${(x2 - minX)}
                      y2=${(y2 - minY)}
                />
            </svg>
        `;
    }

    createRenderRoot() {
        return this;
    }
}