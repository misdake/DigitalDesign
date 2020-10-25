export function component<T extends { new(...args: any[]): {} }>(constructor: T) {
    console.log("Register Component:", constructor.name);
    //把名称和构造函数加入到list或map里，作为总的资源库
    return constructor;
}

export function inputPin(width: number) {
    return function (target: Component, propertyKey: string | symbol) {
        let pin = new InputPin(propertyKey.toString(), width);
        target.inputPins = target.inputPins || [];
        target.inputPins.push(pin);
        // @ts-ignore
        target[propertyKey] = pin;
    };
}

export function outputPin(width: number) {
    return function (target: Component, propertyKey: string | symbol) {
        let pin = new OutputPin(propertyKey.toString(), width);
        target.outputPins = target.outputPins || [];
        target.outputPins.push(pin);
        // @ts-ignore
        target[propertyKey] = pin;
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
        this.toPin.writeByWire(this.fromPin.data); //TODO 检查宽度？
    }
}

export class InputPin {
    readonly name: string;
    readonly width: number;

    constructor(name: string, width: number) { //THINK 最大设置多少宽度
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
}

export class OutputPin {
    readonly name: string;
    readonly width: number;

    constructor(name: string, width: number) { //THINK 最大设置多少宽度
        this.name = name;
        this.width = width;
    }

    data: number;

    write(data: number): void { //THINK 检查宽度？
        this.data = data;
    }
}

export class Component implements LogicRun {
    inputPins: InputPin[];
    outputPins: OutputPin[];

    run() {
        console.log("not implemented");
    };
}