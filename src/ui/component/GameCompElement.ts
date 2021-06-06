import interact from 'interactjs';
import {customElement, html, LitElement, property} from "lit-element";
import "./PinElement";
import {GameComp} from "../../game/GameComp";
import {Game} from "../../game/Game";
import {Events} from "../../util/Events";
import {CELL_SIZE} from "../../util/Constants";

@customElement('gamecomp-element')
export class GameCompElement extends LitElement {

    @property()
    game: Game;
    @property()
    gameComp: GameComp;

    private tx: number;
    private ty: number;

    protected render() {
        this.gameComp.uiElement = this;

        let component = this.gameComp.component;
        let inputPins = this.gameComp.inputPins;
        let outputPins = this.gameComp.outputPins;

        let width = CELL_SIZE * this.gameComp.w;
        let height = CELL_SIZE * Math.max(this.gameComp.h);

        let inputs = inputPins.map(pin => html`<inputpin-element .game=${this.game} .gameComp=${this.gameComp} .gamePin=${pin}></inputpin-element>`);
        let outputs = outputPins.map(pin => html`<outputpin-element .game=${this.game} .gameComp=${this.gameComp} .gamePin=${pin}></outputpin-element>`);

        let tx = this.gameComp.x * CELL_SIZE;
        let ty = this.gameComp.y * CELL_SIZE;
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
            if (!force) {
                this.gameComp.x = x;
                this.gameComp.y = y;
                this.gameComp.fire(Events.COMPONENT_UPDATE, this.gameComp, {x, y});
                this.game.fire(Events.COMPONENT_UPDATE, this.gameComp, {x, y});
            }
            let tx = x * CELL_SIZE;
            let ty = y * CELL_SIZE;
            element.style.transform = `translate(${tx}px, ${ty}px)`;
        }
    }

    updated() {
        let self = this;

        let dragElement = this.getElementsByClassName("component-bg").item(0) as HTMLDivElement;
        let compElement = this.getElementsByClassName("component").item(0) as HTMLDivElement;

        this.updateXY(compElement, this.gameComp.x, this.gameComp.y, true);

        // noinspection JSUnusedGlobalSymbols
        interact(dragElement).draggable({
            listeners: {
                start(_event) {
                    // console.log(event.type, event.target);
                },
                move(event) {
                    self.tx += event.dx;
                    self.ty += event.dy;

                    let x = Math.round(self.tx / CELL_SIZE);
                    let y = Math.round(self.ty / CELL_SIZE);

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