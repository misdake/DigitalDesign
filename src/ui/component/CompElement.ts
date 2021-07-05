import interact from 'interactjs';
import {customElement, html, LitElement, property} from "lit-element";
import "./PinElement";
import {GameComp, GameCompShowMode} from "../../game/GameComp";
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

    protected render() {
        this.gameComp.uiElement = this;

        let component = this.gameComp.component;
        let inputPins = this.gameComp.inputPins;
        let outputPins = this.gameComp.outputPins;

        let width = CELL_SIZE * this.gameComp.w;
        let height = CELL_SIZE * Math.max(this.gameComp.h);

        if (this.gameComp.isTemplate) {
            width = CELL_SIZE * 2;
            height = CELL_SIZE;
        }

        let nameClass: string;
        let typeClass: string;
        let pinsClass: string;

        switch (this.gameComp.showMode) {
            case GameCompShowMode.Name:
                nameClass = "component-name component-name-always";
                typeClass = null;
                pinsClass = "pin-alwayshide";
                break;
            case GameCompShowMode.Type:
                nameClass = null;
                typeClass = "component-type component-type-always";
                pinsClass = "pin-hovershow";
                break;
            case GameCompShowMode.NameType:
                nameClass = "component-name component-name-hoverhide";
                typeClass = "component-type component-type-hovershow";
                pinsClass = "pin-hovershow";
                break;
        }

        let dummyTemplate = html``;
        let nameTemplate = nameClass ? html`
            <div style="pointer-events: none;" class="${nameClass}">${component.name}</div>` : dummyTemplate;
        let typeTemplate = typeClass ? html`
            <div style="pointer-events: none;" class="${typeClass}">${component.type}</div>` : dummyTemplate;

        let inputs = inputPins.map(pin => html`
            <inputpin-element .game=${this.game} .gameComp=${this.gameComp} .gamePin=${pin}></inputpin-element>`);
        let outputs = outputPins.map(pin => html`
            <outputpin-element .game=${this.game} .gameComp=${this.gameComp} .gamePin=${pin}></outputpin-element>`);

        let content = this.gameComp.isTemplate ? html`
            <div style="pointer-events: none;" class="component-type component-type-always">${component.type}</div>
        ` : html`
            ${nameTemplate}
            ${typeTemplate}
            <div style="pointer-events: none;" class="${pinsClass} input-pin-list">${inputs}</div>
            <div style="pointer-events: none;" class="${pinsClass} output-pin-list">${outputs}</div>
        `;

        //transform translate set in updated() callback
        //TODO 如果name===type，那么不使用动画显示name，只固定显示type
        return html`
            <div class="component" style="pointer-events: auto; touch-action: none; width: ${width}px; height: ${height}px;" @click=${() => this.game.editor.selectGameComp(this.gameComp)}>
                <div class="component-bg"></div>
                ${content}
            </div>
        `;
    }

    updated() {
        let self = this;

        let dragElement = this.getElementsByClassName("component-bg").item(0) as HTMLDivElement;
        let compElement = this.getElementsByClassName("component").item(0) as HTMLDivElement;

        self.game.editor.component.tryMoveComponent(self.gameComp, compElement, this.gameComp.x, this.gameComp.y, true);

        if (this.gameComp.movable) {
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

                        if (self.gameComp.isTemplate) {
                            self.game.editor.component.createRealComponentFromTemplate(self.gameComp);
                            self.requestUpdateInternal();
                        }
                        self.game.editor.component.tryMoveComponent(self.gameComp, compElement, x, y);
                    },
                    end(event) {
                        //TODO 再次检查是否可以放下，包括是否在装备栏里面没拿到场地里
                        // console.log("event end", event);
                    },
                },
            });
        }
    }

    createRenderRoot() {
        return this;
    }

}