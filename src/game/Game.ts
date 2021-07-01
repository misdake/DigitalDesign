import {Editor} from "./Editor";
import {GameComp} from "./GameComp";
import {System} from "../logic/System";
import {registerBasicComponents} from "../logic/components/basic";
import {GameWire} from "./GameWire";
import {EventHost} from "../util/EventHost";
import {Component, Wire} from "../logic/Component";
import {ComponentTemplate} from "../logic/ComponentTemplate";
import {PLAYGROUND_WIDTH} from "../util/Constants";

export class Game extends EventHost {
    readonly system: System;
    private mainComponent: Component;

    readonly templates: GameComp[];
    readonly components: GameComp[];
    readonly wires: GameWire[];

    readonly editor: Editor;

    constructor() {
        super();

        this.system = new System();
        registerBasicComponents(this.system);

        this.templates = [];
        this.components = [];
        this.wires = [];

        this.editor = new Editor(this);
    }

    load(template: ComponentTemplate) {
        this.mainComponent = new Component("", true, template, null);
        this.system.setMainComponent(this.mainComponent);

        //TODO 清空界面
        this.templates.length = 0;
        this.components.length = 0;
        this.wires.length = 0;

        this.editMain(main => {
            for (let inputPin of template.inputPins) {
                //TODO 根据pin宽度和类型来决定component类型
                let comp = this.editor.component.createRealComponent({name: "input", type: "pass1", w: 3, h: 1}, -1, 5);
                let fromPin = main.inputPins[inputPin.name];
                let toPin = comp.component.inputPins["in"];
                main.wires.push(new Wire("in_dummy_" + inputPin.name, null, fromPin, comp.component, toPin));
                main.components["in_dummy_" + inputPin.name] = comp.component;
            }
            for (let outputPin of template.outputPins) {
                //TODO 根据pin宽度和类型来决定component类型
                let comp = this.editor.component.createRealComponent({name: "output", type: "pass1", w: 3, h: 1}, PLAYGROUND_WIDTH - 2, 5);
                let fromPin = comp.component.outputPins["out"];
                let toPin = main.outputPins[outputPin.name];
                main.wires.push(new Wire("out_dummy_" + outputPin.name, null, fromPin, comp.component, toPin));
                main.components["out_dummy_" + outputPin.name] = comp.component;
            }
        });
    }

    editMain(mutator: (main: Component) => void) {
        mutator(this.mainComponent);
        console.log("edit main component:", this.mainComponent.components, this.mainComponent.wires);
    }

}