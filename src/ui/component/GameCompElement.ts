import interact from 'interactjs';
import {customElement, html, LitElement, property} from "lit-element";
import "./PinElement";
import {GameComp} from "../../game/GameComp";
import {Game} from "../../game/Game";

@customElement('gamecomp-element')
export class GameCompElement extends LitElement {

    @property()
    game: Game;
    @property()
    gameComp: GameComp;

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

        let inputs = inputPins.map(pin => html`<inputpin-element .game=${this.game} .gameComp=${this.gameComp} .gamePin=${pin}></inputpin-element>`);
        let outputs = outputPins.map(pin => html`<outputpin-element .game=${this.game} .gameComp=${this.gameComp} .gamePin=${pin}></outputpin-element>`);

        let tx = this.gameComp.x * 50;
        let ty = this.gameComp.y * 50;
        this.tx = tx;
        this.ty = ty;

        //transform translate set in updated() callback
        //TODO 如果name===type，那么不使用动画显示name，只固定显示type
        return html`
            <div class="component" style="touch-action: none; width: ${width}px; height: ${height}px;" @click=${() => this.game.editor.selectGameComp(this.gameComp)}>
                <div class="component-bg"></div>
                <div style="pointer-events: none;" class="component-name">${component.name}</div>
                <div style="pointer-events: none;" class="component-type">${component.type}</div>
                <div style="pointer-events: none;" class="input-pin-list">${inputs}</div>
                <div style="pointer-events: none;" class="output-pin-list">${outputs}</div>
            </div>
        `;
    }

    private updateXY(element: HTMLDivElement, x: number, y: number, force: boolean = false) {
        let gameComp = this.gameComp;
        if (force || gameComp.x !== x || gameComp.y !== y) {
            // console.log("updateXY");
            gameComp.x = x;
            gameComp.y = y;
            let tx = x * 50;
            let ty = y * 50;
            element.style.transform = `translate(${tx}px, ${ty}px)`;

            if (!force) {
                //通知，用来更新连线

                //TODO 把上面gameComp的修改放到editor.moveComponent里

                this.game.editor.moveComponent(this.gameComp, x, y);
            }
        }
    }

    updated() {
        let self = this;

        let dragElement = this.getElementsByClassName("component-bg").item(0) as HTMLDivElement;
        let compElement = this.getElementsByClassName("component").item(0) as HTMLDivElement;

        this.updateXY(compElement, this.gameComp.x, this.gameComp.y, true);

        interact(dragElement).draggable({
            listeners: {
                start(event) {
                    // console.log(event.type, event.target);
                },
                move(event) {
                    self.tx += event.dx;
                    self.ty += event.dy;

                    let x = Math.round(self.tx / 50);
                    let y = Math.round(self.ty / 50);

                    //TODO 从Editor走一圈来更新xy，同时限制最大最小值
                    self.updateXY(compElement, x, y);
                },
            },
        });
    }

    createRenderRoot() {
        return this;
    }

}