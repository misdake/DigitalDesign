import {ComponentTemplate, PinType} from "./ComponentTemplate";

export type ComponentGenerator = (name: string, componentLibrary: Map<string, ComponentGenerator>) => Component;

export class Pin {
    readonly name: string;
    readonly width: number;
    readonly type: PinType;
    private data: number;

    constructor(name: string, width: number, type: PinType, data: number) {
        this.name = name;
        this.width = width;
        this.type = type;
        this.data = data;
    }

    read() {
        return this.data; //TODO 检查宽度
    }

    write(data: number, width: number) {
        this.data = data; //TODO 检查宽度
    }

    run() {

    }
}

export class Wire {
    name: string;
    width: number;
    fromComponent: Component;
    fromPin: Pin;
    toComponent: Component;
    toPin: Pin;

    constructor(name: string, width: number, fromComponent: Component, fromPin: Pin, toComponent: Component, toPin: Pin) {
        this.name = name;
        this.width = width;
        this.fromComponent = fromComponent;
        this.fromPin = fromPin;
        this.toComponent = toComponent;
        this.toPin = toPin;
    }

    run() {
        this.toPin.write(this.fromPin.read(), this.width); //TODO 检查宽度
    }
}

export class Component {
    readonly name: string; //TODO 改名?
    readonly isCustom: boolean; //是否是用自定义的Component，如果是=>Component用于连接内部组件，如果不是=>使用run方法执行逻辑
    // template: ComponentTemplate;

    inputPins: Map<string, Pin>;
    components: Map<string, Component>;
    outputPins: Map<string, Pin>;

    wires: Wire[];

    constructor(name: string, isCustom: boolean, template: ComponentTemplate, componentLibrary: Map<string, ComponentGenerator>) {
        this.name = name;
        this.isCustom = isCustom;

        //根据模板设置自己的内容

        // this.template = template;
        this.inputPins = new Map<string, Pin>();
        this.components = new Map<string, Component>();
        this.outputPins = new Map<string, Pin>();
        this.wires = [];

        let components = template.components;
        for (let component of components) {
            let generator = componentLibrary.get(component.type);
            if (!generator) {
                //TODO 没有这个component，报错
            }
            let created = generator(component.name, componentLibrary);
            this.components.set(component.name, created);
        }

        template.inputPins.forEach(input => this.inputPins.set(input.name, new Pin(input.name, input.width, input.type, -1)));
        template.outputPins.forEach(output => this.outputPins.set(output.name, new Pin(output.name, output.width, output.type, -1)));

        template.wires.forEach(w => {
            let fromComponent = w.fromComponent ? this.getComponent(w.fromComponent) : this;
            let fromPin = w.fromComponent ? fromComponent.getOutputPin(w.fromPin) : this.getInputPin(w.fromPin);
            let toComponent = w.toComponent ? this.getComponent(w.toComponent) : this;
            let toPin = w.toComponent ? toComponent.getInputPin(w.toPin) : this.getOutputPin(w.toPin);
            this.wires.push(new Wire(w.name, w.width, fromComponent, fromPin, toComponent, toPin));
        });

    }

    getComponent(name: string) {
        return this.components.get(name); //TODO 检查是否为空
    }

    getInputPin(name: string) {
        return this.inputPins.get(name); //TODO 检查是否为空
    }

    getOutputPin(name: string) {
        return this.outputPins.get(name); //TODO 检查是否为空
    }

    run() {

    }
}