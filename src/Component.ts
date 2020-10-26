let index = 0;

export function component<T extends { new(...args: any[]): {} }>(constructor: T) {
    console.log("Register Component:", constructor.name);
    //把名称和构造函数加入到list或map里，作为总的资源库
    return constructor;
}

export function inputPin(width: number) {
    return function (target: Component, propertyKey: string | symbol) {
        let pin = new InputPin("", propertyKey.toString(), width);
        target.proto_inputPins = target.proto_inputPins || [];
        target.proto_inputPins.push(pin);
    };
}

export function outputPin(width: number) {
    return function (target: Component, propertyKey: string | symbol) {
        let pin = new OutputPin("", propertyKey.toString(), width);
        target.proto_outputPins = target.proto_outputPins || [];
        target.proto_outputPins.push(pin);
    };
}

export interface LogicRun {
    run(): void; //TODO 需要参数，改个名字
}

export class Wire implements LogicRun {
    fromPin: OutputPin;
    fromComponent: Component;

    toPin: InputPin;
    toComponent: Component;

    setFrom(fromPin: OutputPin, fromComponent: Component) {
        this.fromPin = fromPin;
        this.fromComponent = fromComponent;
    }

    setTo(toPin: InputPin, toComponent: Component) {
        this.toPin = toPin;
        this.toComponent = toComponent;
    }

    run() {
        // console.log(`wire ${this.fromPin.name}(${this.fromPin.id}) to ${this.toPin.name}(${this.toPin.id}), ${this.fromPin.data}`);
        this.toPin.writeByWire(this.fromPin.data); //TODO 检查宽度？
    }
}

export class InputPin {
    readonly id: string;
    readonly name: string;
    readonly width: number;

    constructor(id: string, name: string, width: number) { //THINK 最大设置多少宽度
        this.id = id;
        this.name = name;
        this.width = width;
    }

    data: number;

    writeByWire(data: number): void { //THINK 检查宽度？
        this.data = data;
        //TODO 如何通知component？
        //TODO 一个component的所有input都好了之后，就可以开始执行了（其实可以更早，但这个就作为）
    }

    read(): number {
        return this.data;
    }

    read1(): number {
        return this.data ? 1 : 0;
    }
}

export class OutputPin {
    readonly id: string;
    readonly name: string;
    readonly width: number;

    constructor(id: string, name: string, width: number) { //THINK 最大设置多少宽度
        this.id = id;
        this.name = name;
        this.width = width;
    }

    data: number;

    write(data: number): void { //THINK 检查宽度？
        this.data = data;
    }
}

export class Component implements LogicRun { //TODO 名字
    proto_inputPins: InputPin[];
    proto_outputPins: OutputPin[];

    inputPins: InputPin[];
    outputPins: OutputPin[];

    constructor() {
        // console.log("Component construct");
        this.initPins();
    }

    initPins() {
        // console.log("initPins");
        this.inputPins = (this.proto_inputPins || []).map(i => {
            let ip = new InputPin(`input_id_${++index}`, i.name, i.width);
            // @ts-ignore
            this[i.name] = ip;
            return ip;
        });
        this.outputPins = (this.proto_outputPins || []).map(i => {
            let op = new OutputPin(`output_id_${++index}`, i.name, i.width);
            // @ts-ignore
            this[i.name] = op;
            return op;
        });
    }

    run() {
        console.log("not implemented");
    };
}