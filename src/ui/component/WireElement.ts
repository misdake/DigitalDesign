import {customElement, html, LitElement, property} from "lit-element";
import {Game} from "../../game/Game";
import {GameWire} from "../../game/GameWire";
import {Events} from "../../util/Events";
import {CELL_SIZE, WIRE_WIDTH} from "../../util/Constants";

@customElement('wire-element')
export class WireElement extends LitElement {
    @property()
    game: Game;
    @property()
    gameWire: GameWire;

    protected render() {
        this.gameWire.on(Events.WIRE_UPDATE, this, () => this.requestUpdateInternal(), true);

        let {x: x1, y: y1} = this.gameWire.fromPin.getXy();
        let {x: x2, y: y2} = this.gameWire.toPin.getXy();

        let nx1 = (x1 + 0.5) * CELL_SIZE;
        let nx2 = (x2 + 0.5) * CELL_SIZE;
        let ny1 = (y1 + 0.5) * CELL_SIZE;
        let ny2 = (y2 + 0.5) * CELL_SIZE;

        let minX = Math.min(nx1, nx2) - 10;
        let minY = Math.min(ny1, ny2) - 10;
        let maxX = Math.max(nx1, nx2) + 10;
        let maxY = Math.max(ny1, ny2) + 10;

        return html`
            <svg class="pin-svg" width="${(maxX - minX)}" height="${(maxY - minY)}" style="position: absolute; left: ${minX}px; top: ${minY}px; pointer-events: none;">
                <line class="pin-wire" stroke="red" stroke-width="${WIRE_WIDTH}px" stroke-linecap="round"
                      x1=${(nx1 - minX)}
                      y1=${(ny1 - minY)}
                      x2=${(nx2 - minX)}
                      y2=${(ny2 - minY)}
                />
            </svg>
        `;
    }

    createRenderRoot() {
        return this;
    }
}