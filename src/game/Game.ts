import {Editor} from "./Editor";
import {GameComp, GameCompShowMode} from "./GameComp";
import {System} from "../logic/System";
import {registerBasicComponents} from "../logic/components/basic";
import {GameWire} from "./GameWire";
import {EventHost} from "../util/EventHost";
import {Component, Pin, Wire} from "../logic/Component";
import {ComponentTemplate} from "../logic/ComponentTemplate";
import {GAME_WIDTH} from "../util/Constants";

export class Game extends EventHost {
    readonly system: System;
    private mainComponent: Component;
    private dummyPassComponent: Map<Component, Pin>;
    private dummyPassWire: Map<Wire, Pin>;

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
        (window as any).run0 = () => this.run({in: 0});
        (window as any).run1 = () => this.run({in: 1});
    }

    load(template: ComponentTemplate) {
        this.mainComponent = new Component("", true, template, null);
        this.system.setMainComponent(this.mainComponent);

        //TODO 支持二次加载，清空数据，清空界面
        this.templates.length = 0;
        this.components.length = 0;
        this.wires.length = 0;

        this._editMain_editor("load template", main => {
            this.dummyPassComponent = new Map();
            this.dummyPassWire = new Map();

            let inputOffset = 1;
            for (let inputPin of template.inputPins) {
                let comp = this.editor.component.createRealComponent({name: inputPin.name, type: `pass${inputPin.width}`, w: 2, h: 1}, 0, inputOffset);
                inputOffset += inputPin.width;
                comp.showMode = GameCompShowMode.Name;
                comp.movable = false;
                let fromPin = main.inputPins[inputPin.name];
                let toPin = comp.component.inputPins["in"];
                let wire = new Wire(null, fromPin, comp.component, toPin);
                main.wires.push(wire);
                main.components["in_dummy_" + inputPin.name] = comp.component;
                this.dummyPassWire.set(wire, fromPin);
                this.dummyPassComponent.set(comp.component, fromPin);
            }

            let outputOffset = 1;
            for (let outputPin of template.outputPins) {
                let comp = this.editor.component.createRealComponent({name: outputPin.name, type: `pass${outputPin.width}`, w: 2, h: 1}, GAME_WIDTH - 2, outputOffset);
                outputOffset += outputPin.width;
                comp.showMode = GameCompShowMode.Name;
                comp.movable = false;
                let fromPin = comp.component.outputPins["out"];
                let toPin = main.outputPins[outputPin.name];
                let wire = new Wire(comp.component, fromPin, null, toPin);
                main.wires.push(wire);
                main.components["out_dummy_" + outputPin.name] = comp.component;
                this.dummyPassWire.set(wire, toPin);
                this.dummyPassComponent.set(comp.component, fromPin);
            }
        });
    }

    run(input: { [key: string]: number }) {
        this.mainComponent.applyInputValues(input);
        this.system.constructGraph();
        this.system.runLogic();
        return this.mainComponent.getOutputValues();
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

        let validComponents = Object.values(component.components).filter(i => !this.dummyPassComponent.has(i));
        let nameMap: Map<Component, string> = new Map();

        template.components = validComponents.map(i => {
            let size = nameMap.size;
            let name = `comp_${size + 1}`;
            nameMap.set(i, name);
            return {
                name: name,
                type: i.type,
            };
        });
        template.wires = component.wires.filter(i => !this.dummyPassWire.has(i)).map(i => {
            let fromInput = this.dummyPassComponent.has(i.fromComponent);
            let toOutput = this.dummyPassComponent.has(i.toComponent);
            return ({
                name: "wire",
                fromComponent: fromInput ? null : nameMap.get(i.fromComponent),
                fromPin: fromInput ? this.dummyPassComponent.get(i.fromComponent).name : i.fromPin.name,
                toComponent: toOutput ? null : nameMap.get(i.toComponent),
                toPin: toOutput ? this.dummyPassComponent.get(i.toComponent).name : i.toPin.name,
            });
        });

        return template;
    }

    _editMain_editor(title: string, mutator: (main: Component) => void) {
        mutator(this.mainComponent);
        console.log(title, ":", this.mainComponent.components, this.mainComponent.wires);
    }

}