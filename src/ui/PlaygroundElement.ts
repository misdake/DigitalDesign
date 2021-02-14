import {customElement, html, LitElement, property, PropertyValues} from "lit-element";
import "./component/GameCompElement";
import {Game} from "../game/Game";

@customElement('playground-element')
export class PlaygroundElement extends LitElement {
    @property()
    game: Game;


    protected firstUpdated(_changedProperties: PropertyValues) {
        super.firstUpdated(_changedProperties);

        this.game.editor.registerUpdate(() => {
            //TODO 检查是否只进行增删
            console.log("update");
            this.requestUpdate();
        });
    }

    render() {
        //TODO 添加每个gameComp的id
        let components = this.game.components.map(component => html`
            <gamecomp-element .game=${this.game} .gameComp=${component} style="position: absolute;"></gamecomp-element>`);

        console.log("render");
        return html`
            <div class="playground">
                <div class="components">${components}</div>
            </div>
        `;
        //TODO 还有输入输出和连线
    }

    createRenderRoot() {
        return this;
    }

}