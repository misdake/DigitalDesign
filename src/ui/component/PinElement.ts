import {customElement, html, LitElement, property, TemplateResult} from "lit-element";
import {GamePin} from "../../game/GamePin";
import {GameComp} from "../../game/GameComp";
import {Game} from "../../game/Game";

@customElement('inputpin-element')
export class InputPinElement extends LitElement {
    @property()
    game: Game;
    @property()
    gameComp: GameComp;
    @property()
    gamePin: GamePin;

    protected render() {
        let pin = this.gamePin.pin;

        // UI有三个部分：
        // 1.名称，只在hover时显示
        // 2.连线
        // 3.圆圈，有内外两层，点击后选中

        return html`
            <div class="pin input-pin">
                <div class="pin-name inputpin-name">${pin.name}</div>
                <div class="pin-dash inputpin-dash"></div>
                <div class="pin-circle inputpin-circle ${this.game.editor.pin.isSelectedPin(this) ? 'inputpin-circle-selected' : ''}" @click=${() => this.clickCircle()}></div>
            </div>
        `;
    }

    private clickCircle() {
        this.game.editor.pin.selectInputPin(this);
    }

    createRenderRoot() {
        return this;
    }
}

@customElement('outputpin-element')
export class OutputPinElement extends LitElement {
    @property()
    game: Game;
    @property()
    gameComp: GameComp;
    @property()
    gamePin: GamePin;

    protected render() {
        let pin = this.gamePin.pin;
        return html`
            <div class="pin output-pin">
                <div class="pin-name outputpin-name">${pin.name}</div>
                <div class="pin-dash outputpin-dash"></div>
                <div class="pin-circle outputpin-circle ${this.game.editor.pin.isSelectedPin(this) ? 'outputpin-circle-selected' : ''}" @click=${() => this.clickCircle()}></div>
            </div>
        `;
    }

    private clickCircle() {
        this.game.editor.pin.selectOutputPin(this);
    }

    createRenderRoot() {
        return this;
    }
}