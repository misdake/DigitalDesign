import {customElement, html, LitElement, property} from "lit-element";
import {GamePin} from "../../game/GamePin";
import {GameComp} from "../../game/GameComp";
import {Game} from "../../game/Game";
import {Events} from "../../util/Events";

@customElement('inputpin-element')
export class InputPinElement extends LitElement {
    @property()
    game: Game;
    @property()
    gameComp: GameComp;
    @property()
    gamePin: GamePin;

    private circle: SVGLineElement;

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
                <div class="pin-circle inputpin-circle ${this.game.editor.pin.isSelectedPin(this) ? 'inputpin-circle-selected' : ''}" @click=${(event: MouseEvent) => this.leftClick(event)} @contextmenu=${(event: MouseEvent) => this.rightClick(event)}></div>
            </div>
        `;
    }

    private leftClick(event: MouseEvent) {
        this.game.editor.pin.selectInputPin(this);
    }
    private rightClick(event: MouseEvent) {
        event.preventDefault();
        this.game.editor.wire.removeWiresOfPin(this.gamePin);
    }

    updated() {
        this.circle = this.getElementsByClassName("pin-circle")[0] as SVGLineElement;
        this.gamePin.fire(Events.INPUTPIN_UI_CREATED, this);
    }
    updatePinValue() {
        let value = this.gamePin.pin.read();
        this.circle.style.background = value > 0 ? "#4DAA57" : "#BD4F6C"; //TODO 用class和css来实现变色
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

    private circle: SVGLineElement;

    protected render() {
        let pin = this.gamePin.pin;
        return html`
            <div class="pin output-pin">
                <div class="pin-name outputpin-name">${pin.name}</div>
                <div class="pin-dash outputpin-dash"></div>
                <div class="pin-circle outputpin-circle ${this.game.editor.pin.isSelectedPin(this) ? 'outputpin-circle-selected' : ''}" @click=${(event: MouseEvent) => this.leftClick(event)} @contextmenu=${(event: MouseEvent) => this.rightClick(event)}></div>
            </div>
        `;
    }

    private leftClick(event: MouseEvent) {
        this.game.editor.pin.selectOutputPin(this);
    }
    private rightClick(event: MouseEvent) {
        event.preventDefault();
        this.game.editor.wire.removeWiresOfPin(this.gamePin);
    }

    updated() {
        this.circle = this.getElementsByClassName("pin-circle")[0] as SVGLineElement;
        this.gamePin.fire(Events.OUTPUTPIN_UI_CREATED, this);
    }
    updatePinValue() {
        let value = this.gamePin.pin.read();
        this.circle.style.background = value > 0 ? "#4DAA57" : "#BD4F6C"; //TODO 用class和css来实现变色
    }

    createRenderRoot() {
        return this;
    }
}