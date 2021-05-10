import {customElement, html, LitElement, property} from "lit-element";
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

    render() {
        let pin = this.gamePin.pin;

        // UI有三个部分：
        // 1.名称，只在hover时显示
        // 2.连线
        // 3.圆圈，有内外两层，点击后选中

        return html`
            <div class="pin input-pin">
                <div class="pin-name inputpin-name">${pin.name}</div>
                <div class="pin-dash inputpin-dash"></div>
                <div class="pin-circle inputpin-circle ${this.game.editor.isSelectedPin(this) ? 'inputpin-circle-selected' : ''}" @click=${() => this.clickCircle()}></div>
            </div>
        `;
    }

    public getPosition() { //TODO 这个getPosition是不是放在GamePin更好
        let centerX = this.gameComp.x * 50 - 25;
        let centerY = this.gameComp.y * 50 + this.gamePin.index * 50 + 25;
        return {x: centerX, y: centerY};
    }

    private clickCircle() {
        this.game.editor.selectInputPin(this);
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

    render() {
        let pin = this.gamePin.pin;
        return html`
            <div class="pin output-pin">
                <div class="pin-name outputpin-name">${pin.name}</div>
                <div class="pin-dash outputpin-dash"></div>
                <div class="pin-circle outputpin-circle ${this.game.editor.isSelectedPin(this) ? 'outputpin-circle-selected' : ''}" @click=${() => this.clickCircle()}></div>
            </div>
        `;
    }

    private clickCircle() {
        this.game.editor.selectOutputPin(this);
    }

    createRenderRoot() {
        return this;
    }
}