import {Editor} from "./Editor";
import {GameComp, GameCompShowMode} from "./GameComp";
import {System} from "../logic/System";
import {registerBasicComponents} from "../logic/components/basic";
import {GameWire} from "./GameWire";
import {EventHost} from "../util/EventHost";
import {Component, Pin, Wire} from "../logic/Component";
import {ComponentTemplate, PinType} from "../logic/ComponentTemplate";
import {CELL_SIZE, GAME_WIDTH} from "../util/Constants";
import {render} from "lit-html";
import {html} from "lit-element";
import {Events} from "../util/Events";

export class Game extends EventHost {
    readonly system: System;
    private mainComponent: Component;
    private dummyPassComponent: Map<Component, Pin>;
    private dummyPassWire: Map<Wire, Pin>;

    readonly templates: GameComp[];
    readonly components: GameComp[];
    readonly wires: GameWire[];

    private inputUiMap: Map<string, HTMLInputElement>;
    private outputUiMap: Map<string, HTMLDivElement>;

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
        (window as any).run = () => this.run();

        let callback = (_obj: any) => {
            setTimeout(() => {
                this.run();
            });
        };
        // this.on(Events.COMPONENT_ADD, this, callback);
        this.on(Events.COMPONENT_REMOVE, this, callback);
        // this.on(Events.COMPONENT_UPDATE, this, callback);
        this.on(Events.WIRE_ADD, this, callback);
        this.on(Events.WIRE_REMOVE, this, callback);
        this.on(Events.WIRES_REMOVE, this, callback);
        // this.on(Events.WIRE_UPDATE, this, callback);
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

            this.inputUiMap = new Map();
            let inputOffset = 1;
            for (let inputPin of template.inputPins) {
                let comp = this.editor.component.createRealComponent({name: inputPin.name, type: `pass${inputPin.width}`, w: 2, h: 1}, 0, inputOffset);
                inputOffset += inputPin.width + 1;
                comp.showMode = GameCompShowMode.Name;
                comp.movable = false;
                let fromPin = main.inputPins[inputPin.name];
                let toPin = comp.component.inputPins["in"];
                let wire = new Wire(null, fromPin, comp.component, toPin);
                main.wires.push(wire);
                main.components["in_dummy_" + inputPin.name] = comp.component;
                this.dummyPassWire.set(wire, fromPin);
                this.dummyPassComponent.set(comp.component, fromPin);

                comp.onCreatedUi = element => {
                    let div = element.getElementsByClassName("component-placeholder")[0] as HTMLDivElement;
                    div.style.display = "block";
                    div.style.left = "0";

                    //TODO support >1 width
                    let onChange = (event: InputEvent) => {
                        let target = event.target as HTMLInputElement;
                        fromPin.write(target.checked ? 1 : 0, 1);

                        this.run();
                    };
                    fromPin.write(0, 1);

                    switch (inputPin.type) {
                        case PinType.BOOL:
                            render(html`<input class="input-checkbox" @change=${(event: InputEvent) => onChange(event)} type="checkbox"/>`, div);
                            this.inputUiMap.set(fromPin.name, div.getElementsByClassName("input-checkbox")[0] as HTMLInputElement);
                            break;
                        case PinType.UNSIGNED:
                            //TODO
                            break;
                        case PinType.SIGNED:
                            //TODO
                            break;
                    }
                };
            }

            this.outputUiMap = new Map();
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

                comp.onCreatedUi = element => {
                    let div = element.getElementsByClassName("component-placeholder")[0] as HTMLDivElement;
                    div.style.display = "block";
                    div.style.right = "0";

                    //TODO 搞个好看一点儿的东西
                    div.style.width = `${CELL_SIZE}px`;
                    div.style.background = "#f00";

                    this.outputUiMap.set(toPin.name, div);
                };
            }
        });
    }

    run(input?: { [key: string]: number }) {
        if (!input) {
            input = this.mainComponent.getInputValues();
        }

        this.mainComponent.clear0();
        this.mainComponent.applyInputValues(input);

        this.system.constructGraph();
        this.system.runLogic();

        let outputValues = this.mainComponent.getOutputValues();
        if (this.outputUiMap) {
            for (let key in outputValues) {
                let r = this.outputUiMap.get(key);
                //TODO support other types
                if (r) {
                    r.style.background = outputValues[key] > 0 ? "#0f0" : "#f00";
                }
            }
        }
        console.log("run!", outputValues);
        return outputValues;
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