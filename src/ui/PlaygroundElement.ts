import {customElement, html, LitElement, property, PropertyValues} from "lit-element";
import "./component/CompElement";
import "./component/PinElement";
import "./component/WireElement";
import {Game} from "../game/Game";
import {Events} from "../util/Events";
import {GameComp} from "../game/GameComp";
import {CELL_SIZE, GAME_HEIGHT, GAME_WIDTH, PLAYGROUND_TOP} from "../util/Constants";

@customElement('playground-element')
export class PlaygroundElement extends LitElement {
    @property()
    game: Game;

    protected firstUpdated(_changedProperties: PropertyValues) {
        super.firstUpdated(_changedProperties);

        let callback = (_obj: any) => {
            this.requestUpdateInternal();
        };
        this.game.on(Events.COMPONENT_ADD, this, callback, false, false);
        this.game.on(Events.COMPONENT_REMOVE, this, callback, false, false);
        this.game.on(Events.COMPONENT_UPDATE, this, callback, false, false);
        this.game.on(Events.WIRE_ADD, this, callback, false, false);
        this.game.on(Events.WIRE_REMOVE, this, callback, false, false);
        this.game.on(Events.WIRE_UPDATE, this, callback, false, false);
    }

    protected render() {
        let source: GameComp[] = [];
        source.push(...this.game.templates);
        source.push(...this.game.components);
        source.sort((a, b) => a.id - b.id);

        let components = source.map(component => html`
            <gamecomp-element id="gameComp-${component.id}" .game=${this.game} .gameComp=${component} style="position: absolute; pointer-events: none;"/>`);
        let wires = this.game.wires.map(wire => html`
            <wire-element .gam=${this.game} .gameWire=${wire}></wire-element>`);

        let width = CELL_SIZE * GAME_WIDTH;
        let height = CELL_SIZE * GAME_HEIGHT;
        let top = CELL_SIZE * PLAYGROUND_TOP;

        return html`
            <div id="playground" style="width: ${width}px; height: ${height}px;">
                <div style="position: relative; top: ${top}px;">
                    <div class="components">${components}</div>
                    <div class="wires">${wires}</div>
                </div>
            </div>
        `;
    }

    createRenderRoot() {
        return this;
    }

}