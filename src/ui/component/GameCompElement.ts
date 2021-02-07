import {customElement, html, LitElement, property} from "lit-element";
import {Component} from "../../logic/Component";
import "./PinElement";
import {GameComp} from "../../game/GameComp";

@customElement('gamecomp-element')
export class GameCompElement extends LitElement {

    @property()
    gameComp: GameComp;

    render() {
        this.gameComp.uiElement = this;

        let component = this.gameComp.component;
        let inputPins = Object.values(component.inputPins);
        let outputPins = Object.values(component.outputPins);

        let width = 50 * this.gameComp.w;
        let height = 50 * Math.max(this.gameComp.h, 2, inputPins.length, outputPins.length); //TODO 从component获取

        let inputs = inputPins.map(pin => html`<inputpin-element .pin=${pin}></inputpin-element>`);
        let outputs = outputPins.map(pin => html`<outputpin-element .pin=${pin}></outputpin-element>`);

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

    createRenderRoot() {
        return this;
    }

}