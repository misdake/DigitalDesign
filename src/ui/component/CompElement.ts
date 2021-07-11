import interact from 'interactjs';
import {customElement, html, LitElement, property} from "lit-element";
import "./PinElement";
import {GameComp, GameCompShowMode} from "../../game/GameComp";
import {Game} from "../../game/Game";
import {CELL_SIZE} from "../../util/Constants";
import {Events} from "../../util/Events";

@customElement('gamecomp-element')
export class CompElement extends LitElement {

    @property()
    game: Game;
    @property()
    gameComp: GameComp;

    private tx: number;
    private ty: number;

    protected render() {
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
        return html`
            <div class="component-move-target" style="z-index: 999; position: absolute; pointer-events: auto; box-sizing: border-box; margin: -2px; touch-action: none; width: ${width + CELL_SIZE * 2 + 4}px; height: ${height + 4}px; border: 2px solid;"></div>
            <div class="component" style="position: relative; pointer-events: auto; touch-action: none; width: ${width}px; height: ${height}px;" @click=${() => this.game.editor.selectGameComp(this.gameComp)}>
                <div class="component-bg"></div>
                ${content}
                <div class="component-placeholder" style="display: none; position: absolute; top: 0; align-content: center; line-height: ${CELL_SIZE}px; height: ${CELL_SIZE}px;"></div>
            </div>
        `;
    }

    private dragElement: HTMLDivElement;
    private compElement: HTMLDivElement;
    private targetElement: HTMLDivElement;

    updated() {
        let self = this;

        let dragElement = this.getElementsByClassName("component-bg").item(0) as HTMLDivElement;
        let compElement = this.getElementsByClassName("component").item(0) as HTMLDivElement;
        let targetElement = this.getElementsByClassName("component-move-target").item(0) as HTMLDivElement;
        this.dragElement = dragElement;
        this.compElement = compElement;
        this.targetElement = targetElement;

        targetElement.style.display = "none";
        self.game.editor.component.tryMoveComponent(self.gameComp, compElement, targetElement, this.gameComp.x, this.gameComp.y, true);

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
                        // targetElement.style.display = "block"; //will be set in tryMoveComponent
                        let dx = event.client.x - self.tx;
                        let dy = event.client.y - self.ty;
                        let x = Math.round(dx / CELL_SIZE);
                        let y = Math.round(dy / CELL_SIZE);

                        if (self.gameComp.isTemplate) {
                            self.game.editor.component.createRealComponentFromTemplate(self.gameComp);
                            self.requestUpdateInternal();
                        }
                        self.game.editor.component.tryMoveComponent(self.gameComp, compElement, targetElement, x, y);
                    },
                    end(event) {
                        targetElement.style.display = "none";
                        let dx = event.client.x - self.tx;
                        let dy = event.client.y - self.ty;
                        let x = Math.round(dx / CELL_SIZE);
                        let y = Math.round(dy / CELL_SIZE);

                        if (!self.gameComp.isTemplate) {
                            if (self.game.editor.component.testInTrash(self.gameComp, x, y)) {
                                self.game.editor.component.removeRealComponent(self.gameComp);
                            }
                        }

                        //TODO 再次检查是否可以放下，如果不能就删掉
                        // console.log("event end", event);
                    },
                },
            });
        }

        this.gameComp.fire(Events.COMPONENT_UI_CREATED, this);
    }

    createRenderRoot() {
        return this;
    }

}