import {customElement, html, LitElement, property} from "lit-element";
import {GamePin} from "../../game/GamePin";
import interact from "interactjs";
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

        //TODO render时初始化line

        return html`
            <div class="pin input-pin">
                <div class="pin-name inputpin-name">${pin.name}</div>
                <div class="pin-dash inputpin-dash"></div>
                <div class="pin-circle inputpin-circle"></div>
                <svg class="pin-svg" width="100" height="100" style="position: absolute; left: -25px; top: 25px;">
                    <line class="pin-wire" stroke="red" stroke-width="5px"/>
                </svg>
            </div>
        `;
        //TODO 连线是否要单独放到一个地方去？这样便于控制zindex
    }

    private getPosition() {
        let centerX = this.gameComp.x * 50 - 25;
        let centerY = this.gameComp.y * 50 + this.gamePin.index * 50 + 25;
        return {x: centerX, y: centerY};
    }

    updated() {
        let self = this;

        let circleElement = this.getElementsByClassName("pin-circle").item(0) as HTMLDivElement;

        let svgElement = this.getElementsByClassName("pin-svg").item(0) as SVGLineElement;
        let wireElement = this.getElementsByClassName("pin-wire").item(0) as SVGLineElement;

        //TODO 注册这个wire，包装为要等待生成的GameWire

        interact(circleElement).draggable({
            listeners: {
                start(event) {
                    console.log(event.type, event.target);
                },
                move(event) {

                    //TODO 设置图像宽高、left+right或transform

                    let {x, y} = self.getPosition();
                    console.log("event", x, y, event);
                    //TODO 测试clientXY在有父元素、有其他高层元素的情况下是否稳定

                    wireElement.setAttributeNS(null, "x1", `0`);
                    wireElement.setAttributeNS(null, "y1", `0`);
                    wireElement.setAttributeNS(null, "x2", `${event.clientX - x}`);
                    wireElement.setAttributeNS(null, "y2", `${event.clientY - y}`);
                },
            },
        });
    }

    createRenderRoot() {
        return this;
    }
}

@customElement('outputpin-element')
export class OutputPinElement extends LitElement {
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
                <div class="pin-circle outputpin-circle"></div>
            </div>
        `;
    }

    //TODO 支持作为dropZone，拖拽时显示辅助光圈

    createRenderRoot() {
        return this;
    }
}