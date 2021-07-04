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
    private dummyPassComponent: Set<Component>;
    private dummyPassWire: Set<Wire>;

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


        (window as any).save = () => this.save("out");
    }

    load(template: ComponentTemplate) {
        this.mainComponent = new Component("", true, template, null);
        this.system.setMainComponent(this.mainComponent);

        //TODO 清空界面
        this.templates.length = 0;
        this.components.length = 0;
        this.wires.length = 0;

        this._editMain_editor(main => {
            this.dummyPassComponent = new Set<Component>();
            this.dummyPassWire = new Set<Wire>();
            for (let inputPin of template.inputPins) {
                //TODO 根据pin宽度和类型来决定component类型
                let comp = this.editor.component.createRealComponent({name: "input", type: "pass1", w: 3, h: 1}, -1, 5);
                let fromPin = main.inputPins[inputPin.name];
                let toPin = comp.component.inputPins["in"];
                let wire = new Wire(null, fromPin, comp.component, toPin);
                main.wires.push(wire);
                main.components["in_dummy_" + inputPin.name] = comp.component;
                this.dummyPassWire.add(wire);
                this.dummyPassComponent.add(comp.component);
            }
            for (let outputPin of template.outputPins) {
                //TODO 根据pin宽度和类型来决定component类型
                let comp = this.editor.component.createRealComponent({name: "output", type: "pass1", w: 3, h: 1}, PLAYGROUND_WIDTH - 2, 5);
                let fromPin = comp.component.outputPins["out"];
                let toPin = main.outputPins[outputPin.name];
                let wire = new Wire(comp.component, fromPin, null, toPin);
                main.wires.push(wire);
                main.components["out_dummy_" + outputPin.name] = comp.component;
                this.dummyPassWire.add(wire);
                this.dummyPassComponent.add(comp.component);
            }
        });
    }

    save(typeName: string) {
        let template = new ComponentTemplate();
        template.type = typeName;

        let component = this.mainComponent;
        template.inputPins = Object.values(component.inputPins).map(i => ({
            name: i.name,
            width: i.width,
            type: i.type,
        }));
        template.outputPins = Object.values(component.outputPins).map(i => ({
            name: i.name,
            width: i.width,
            type: i.type,
        }));
        template.components = Object.values(component.components).filter(i => !this.dummyPassComponent.has(i)).map(i => ({
            name: i.name,
            type: i.type,
        }));
        template.wires = component.wires.filter(i => !this.dummyPassWire.has(i)).map(i => ({
            name: "wire",
            fromComponent: this.dummyPassComponent.has(i.fromComponent) ? null : i.fromComponent.name,
            fromPin: i.fromPin.name,
            toComponent: this.dummyPassComponent.has(i.toComponent) ? null : i.toComponent.name,
            toPin: i.toPin.name,
        }));

        console.log(template);

        return template;
    }

    _editMain_editor(mutator: (main: Component) => void) {
        mutator(this.mainComponent);
        console.log("edit main component:", this.mainComponent.components, this.mainComponent.wires);
    }

}