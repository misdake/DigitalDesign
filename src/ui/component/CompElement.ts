import interact from 'interactjs';
import {customElement, html, LitElement, property} from "lit-element";
import "./PinElement";
import {GameComp} from "../../game/GameComp";
import {Game} from "../../game/Game";
import {Events} from "../../util/Events";
import {CELL_SIZE} from "../../util/Constants";

@customElement('gamecomp-element')
export class CompElement extends LitElement {

    @property()
    game: Game;
    @property()
    gameComp: GameComp;

    private tx: number;
    private ty: number;

    @property()
    smallMode: boolean = true; //TODO

    protected render() {
        this.gameComp.uiElement = this;

        let component = this.gameComp.component;
        let inputPins = this.gameComp.inputPins;
        let outputPins = this.gameComp.outputPins;

        let width = CELL_SIZE * this.gameComp.w;
        let height = CELL_SIZE * Math.max(this.gameComp.h);

        if (this.smallMode) {
            width = CELL_SIZE * 2;
            height = CELL_SIZE;
        }

        let inputs = inputPins.map(pin => html`
            <inputpin-element .game=${this.game} .gameComp=${this.gameComp} .gamePin=${pin}></inputpin-element>`);
        let outputs = outputPins.map(pin => html`
            <outputpin-element .game=${this.game} .gameComp=${this.gameComp} .gamePin=${pin}></outputpin-element>`);

        let content = this.smallMode ? html`
            <div style="pointer-events: none;" class="component-type component-type-always">${component.type}</div>
        ` : html`
            <div style="pointer-events: none;" class="component-name">${component.name}</div>
            <div style="pointer-events: none;" class="component-type">${component.type}</div>
            <div style="pointer-events: none;" class="input-pin-list">${inputs}</div>
            <div style="pointer-events: none;" class="output-pin-list">${outputs}</div>
        `;

        //transform translate set in updated() callback
        //TODO 如果name===type，那么不使用动画显示name，只固定显示type
        return html`
            <div class="component" style="touch-action: none; width: ${width}px; height: ${height}px;" @click=${() => this.game.editor.selectGameComp(this.gameComp)}>
                <div class="component-bg"></div>
                ${content}
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
                start(event) {
                    //保存点击位置的相对偏移
                    self.tx = event.client.x - self.gameComp.x * CELL_SIZE;
                    self.ty = event.client.y - self.gameComp.y * CELL_SIZE;
                },
                move(event) {
                    let dx = event.client.x - self.tx;
                    let dy = event.client.y - self.ty;

                    let x = Math.round(dx / CELL_SIZE);
                    let y = Math.round(dy / CELL_SIZE);

                    //TODO 暂时留上3格给工具栏，到下面就放大，回不到上面
                    if (y >= 3 && self.smallMode) {
                        self.smallMode = false;
                        self.updateXY(compElement, x, y);
                    } else if (y < 3 && self.smallMode) {
                        self.updateXY(compElement, x, y);
                    } else if (y >= 3 && !self.smallMode) {
                        self.updateXY(compElement, x, y);
                    }
                },
                end(event) {
                    //TODO 再次检查是否可以放下，包括是否在装备栏里面没拿到场地里
                    // console.log("event end", event);
                },
            },
        });
    }

    createRenderRoot() {
        return this;
    }

}