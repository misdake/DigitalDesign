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
                <div style="position: absolute; background: #000; opacity: 0.5; left: -20px; width: 20px; height: 2px; top: 24px;"></div>
                <div style="position: absolute; background: #000; opacity: 0.5; left: -40px; width: 20px; height: 20px; border-radius: 10px; top: 15px;"></div>
                <div style="position: absolute; background: #fff; opacity: 0.5; left: -38px; width: 16px; height: 16px; border-radius: 8px; top: 17px;"></div>
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
                <div style="position: absolute; background: #000; opacity: 0.5; right: -20px; width: 20px; height: 2px; top: 24px;"></div>
                <div style="position: absolute; background: #000; opacity: 0.5; right: -40px; width: 20px; height: 20px; border-radius: 10px; top: 15px;"></div>
                <div style="position: absolute; background: #fff; opacity: 0.5; right: -38px; width: 16px; height: 16px; border-radius: 8px; top: 17px;"></div>
            </div>
        `;
    }

    createRenderRoot() {
        return this;
    }
}