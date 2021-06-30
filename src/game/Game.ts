import {Editor} from "./Editor";
import {GameComp} from "./GameComp";
import {System} from "../logic/System";
import {registerBasicComponents} from "../logic/components/basic";
import {GameWire} from "./GameWire";
import {EventHost} from "../util/EventHost";
import {Component} from "../logic/Component";
import {ComponentTemplate} from "../logic/ComponentTemplate";
import {PLAYGROUND_WIDTH} from "../util/Constants";

export class Game extends EventHost {
    readonly system: System;

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
        let component = new Component("", true, template, null);
        this.system.setMainComponent(component);

        //TODO 清空界面
        this.templates.length = 0;
        this.components.length = 0;
        this.wires.length = 0;

        //TODO 初始化input和output的pin
        for (let inputPin of template.inputPins) {
            this.editor.component.createRealComponent({name: "input", type: "pass1", w: 3, h: 1}, -1, 5);
        }
        for (let outputPin of template.outputPins) {
            this.editor.component.createRealComponent({name: "output", type: "pass1", w: 3, h: 1}, PLAYGROUND_WIDTH - 2, 5);
        }
    }

}