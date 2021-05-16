import {customElement, html, LitElement, property} from "lit-element";
import {Game} from "../../game/Game";
import {GameWire} from "../../game/GameWire";

@customElement('wire-element')
export class WireElement extends LitElement {
    @property()
    game: Game;
    @property()
    gameWire: GameWire;

    render() {
        let {x: x1, y: y1} = this.gameWire.fromPin.getXy();
        let {x: x2, y: y2} = this.gameWire.toPin.getXy();

        x1 = x1 * 50 + 25;
        x2 = x2 * 50 + 25;
        y1 = y1 * 50 + 25;
        y2 = y2 * 50 + 25;

        let minX = Math.min(x1, x2) - 10;
        let minY = Math.min(y1, y2) - 10;
        let maxX = Math.max(x1, x2) + 10;
        let maxY = Math.max(y1, y2) + 10;

        return html`
            <svg class="pin-svg" width="${(maxX - minX)}" height="${(maxY - minY)}" style="position: absolute; left: ${minX}px; top: ${minY}px; pointer-events: none;">
                <line class="pin-wire" stroke="red" stroke-width="5px"
                      x1=${(x1 - minX)}
                      y1=${(y1 - minY)}
                      x2=${(x2 - minX)}
                      y2=${(y2 - minY)}
                />
            </svg>
        `;
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
        // console.log("wireElement", wireElement);
        // wireElement.setAttributeNS(null, "x1", `${this.x1}`);
        // wireElement.setAttributeNS(null, "y1", `${this.y1}`);
        // wireElement.setAttributeNS(null, "x2", `${this.x2}`);
        // wireElement.setAttributeNS(null, "y2", `${this.y2}`);
        //         },
        //     },
        // });
    }

    createRenderRoot() {
        return this;
    }
}