import {customElement, html, LitElement, property} from "lit-element";
import {GamePin} from "../../game/GamePin";
import {GameComp} from "../../game/GameComp";
import {Game} from "../../game/Game";
import {GameWire} from "../../game/GameWire";

@customElement('wire-element')
export class WireElement extends LitElement {
    @property()
    game: Game;
    @property()
    fromPin: GamePin;
    @property()
    toPin: GamePin;
    @property()
    gameWire: GameWire;

    render() {
        let wire = this.gameWire;

        //TODO render时初始化line

        //TODO 根据

        return html`
            <svg class="pin-svg" width="100" height="100" style="position: absolute; left: -25px; top: 25px;">
                <line class="pin-wire" stroke="red" stroke-width="5px"/>
            </svg>
        `;
        //TODO 连线是否要单独放到一个地方去？这样便于控制zindex
    }

    updated() {
        // let self = this;
        //
        // let circleElement = this.getElementsByClassName("pin-circle").item(0) as HTMLDivElement;
        //
        // let svgElement = this.getElementsByClassName("pin-svg").item(0) as SVGLineElement;
        // let wireElement = this.getElementsByClassName("pin-wire").item(0) as SVGLineElement;
        //
        // //TODO 注册这个wire，包装为要等待生成的GameWire
        //
        // interact(circleElement).draggable({
        //     listeners: {
        //         start(event) {
        //             console.log(event.type, event.target);
        //         },
        //         move(event) {
        //
        //             //TODO 设置图像宽高、left+right或transform
        //
        //             let {x, y} = self.getPosition();
        //             console.log("event", x, y, event);
        //             //TODO 测试clientXY在有父元素、有其他高层元素的情况下是否稳定
        //
        //             wireElement.setAttributeNS(null, "x1", `0`);
        //             wireElement.setAttributeNS(null, "y1", `0`);
        //             wireElement.setAttributeNS(null, "x2", `${event.clientX - x}`);
        //             wireElement.setAttributeNS(null, "y2", `${event.clientY - y}`);
        //         },
        //     },
        // });
    }

    createRenderRoot() {
        return this;
    }
}