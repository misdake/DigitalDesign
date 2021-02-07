import interact from 'interactjs';
import {customElement, html, LitElement, property} from "lit-element";
import "./PinElement";
import {GameComp} from "../../game/GameComp";

@customElement('gamecomp-element')
export class GameCompElement extends LitElement {

    @property()
    gameComp: GameComp;

    private element: HTMLDivElement;
    private tx: number;
    private ty: number;

    render() {
        this.gameComp.uiElement = this;

        let component = this.gameComp.component;
        let inputPins = this.gameComp.inputPins;
        let outputPins = this.gameComp.outputPins;

        //TODO 这个50改为从某个全局或传入属性获取获取，但是CSS里也有关于50的数据，包括pin的头的长度宽度等
        let width = 50 * this.gameComp.w;
        let height = 50 * Math.max(this.gameComp.h);

        let inputs = inputPins.map(pin => html`<inputpin-element .gamePin=${pin}></inputpin-element>`);
        let outputs = outputPins.map(pin => html`<outputpin-element .gamePin=${pin}></outputpin-element>`);

        let tx = this.gameComp.x * 50;
        let ty = this.gameComp.y * 50;
        this.tx = tx;
        this.ty = ty;

        //transform translate set in updated() callback
        return html`
            <div class="component" style="width: ${width}px; height: ${height}px;">
                <div class="component-bg"></div>
                <div class="component-name">${component.name}</div>
                <div class="component-type">${component.type}</div>
                <div class="input-pin-list">${inputs}</div>
                <div class="output-pin-list">${outputs}</div>
            </div>
        `;
    }

    private static updateXY(element: HTMLDivElement, gameComp: GameComp, x: number, y: number, force: boolean = false) {
        if (force || gameComp.x !== x || gameComp.y !== y) {
            // console.log("updateXY");
            gameComp.x = x;
            gameComp.y = y;
            let tx = x * 50;
            let ty = y * 50;
            element.style.transform = `translate(${tx}px, ${ty}px)`;

            if (!force) {
                //TODO 通知，用来更新连线
            }
        }
    }

    updated() {
        let self = this;

        let element = this.getElementsByClassName("component").item(0) as HTMLDivElement;
        if (this.element !== element) {
            this.element = element;

            GameCompElement.updateXY(element, this.gameComp, this.gameComp.x, this.gameComp.y, true);

            interact(element).draggable({
                listeners: {
                    start(event) {
                        console.log(event.type, event.target);
                    },
                    move(event) {
                        self.tx += event.dx;
                        self.ty += event.dy;

                        let x = Math.round(self.tx / 50);
                        let y = Math.round(self.ty / 50);

                        //TODO 限制最大最小值
                        GameCompElement.updateXY(element, self.gameComp, x, y);
                    },
                },
            });
        }
    }

    createRenderRoot() {
        return this;
    }

}