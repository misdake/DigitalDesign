import {customElement, html, LitElement, property, PropertyValues} from "lit-element";
import "./component/GameCompElement";
import "./component/PinElement";
import "./component/WireElement";
import {Game} from "../game/Game";

@customElement('playground-element')
export class PlaygroundElement extends LitElement {
    @property()
    game: Game;

    protected firstUpdated(_changedProperties: PropertyValues) {
        super.firstUpdated(_changedProperties);

        let callback = (_obj : any) => {
            console.log("update in PlaygroundElement");
            this.requestUpdateInternal();
        };
        this.game.on("component_add", this, callback);
        this.game.on("component_remove", this, callback);
        this.game.on("component_update", this, callback);
        this.game.on("wire_add", this, callback);
        this.game.on("wire_remove", this, callback);
        this.game.on("wire_update", this, callback);
    }

    protected render() {
        let components = this.game.components.map(component => html`<gamecomp-element id="gameComp_${component.id}" .game=${this.game} .gameComp=${component} style="position: absolute;" />`);
        let wires = this.game.wires.map(wire => html`<wire-element .gam=${this.game} .gameWire=${wire} />`);

        return html`
            <div class="playground">
                <div class="components">${components}</div>
                <div class="wires">${wires}</div>
            </div>
        `;
    }

    createRenderRoot() {
        return this;
    }

}