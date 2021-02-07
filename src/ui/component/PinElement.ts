import {customElement, html, LitElement, property} from "lit-element";
import {GamePin} from "../../game/GamePin";

@customElement('inputpin-element')
export class InputPinElement extends LitElement {
    @property()
    gamePin: GamePin;

    render() {
        let pin = this.gamePin.pin;
        return html`
            <div class="pin input-pin">
                <div class="pin-name inputpin-name">${pin.name}</div>
                <div style="position: absolute; background: #ccc; left: -15px; width: 15px; height: 2px; top: 24px;"></div>
                <div style="position: absolute; background: #fff; left: -35px; width: 20px; height: 20px; border: 2px #ccc solid; box-sizing: border-box; border-radius: 10px; top: 15px;"></div>
            </div>
        `;
    }

    createRenderRoot() {
        return this;
    }
}

@customElement('outputpin-element')
export class OutputPinElement extends LitElement {
    @property()
    gamePin: GamePin;

    render() {
        let pin = this.gamePin.pin;
        return html`
            <div class="pin output-pin">
                <div class="pin-name outputpin-name">${pin.name}</div>
                <div style="position: absolute; background: #ccc; right: -15px; width: 15px; height: 2px; top: 24px;"></div>
                <div style="position: absolute; background: #fff; right: -35px; width: 20px; height: 20px; border: 2px #ccc solid; box-sizing: border-box; border-radius: 10px; top: 15px;"></div>
            </div>
        `;
    }

    createRenderRoot() {
        return this;
    }
}