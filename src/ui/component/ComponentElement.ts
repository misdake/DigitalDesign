import {customElement, html, LitElement, property} from "lit-element";
import {Component} from "../../logic/Component";
import "./PinElement"

@customElement('component-element')
export class ComponentElement extends LitElement {

    @property()
    private component: Component;

    render() {
        let inputPins = Object.values(this.component.inputPins);
        let outputPins = Object.values(this.component.outputPins);

        let width = 300; //TODO 从component获取
        let height = 50 * Math.max(2, inputPins.length, outputPins.length); //TODO 从component获取

        let inputs = inputPins.map(pin => html`<inputpin-element .pin=${pin}></inputpin-element>`);
        let outputs = outputPins.map(pin => html`<outputpin-element .pin=${pin}></outputpin-element>`);

        return html`
            <div class="component" style="width: ${width}px; height: ${height}px;">
                <div class="component-name">${this.component.name}</div>
                <div class="component-type">${this.component.type}</div>
                <div class="input-pin-list">${inputs}</div>
                <div class="output-pin-list">${outputs}</div>
            </div>
        `;
    }

    createRenderRoot() {
        return this;
    }

}