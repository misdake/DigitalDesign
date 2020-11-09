let index = 0;

export function component<T extends { new(...args: any[]): {} }>(constructor: T) {
    console.log("Register Component:", constructor.name);
    //把名称和构造函数加入到list或map里，作为总的资源库
    return constructor;
}

export function inputPin(width: number, name: string = null) {
    return function (target: Component, propertyKey: string | symbol) {
        let pin = new InputPin("", name || propertyKey.toString(), width, null); //component不存在，会在从proto复制数据的时候设置到新的pin中
        target.proto_inputPins = target.proto_inputPins || [];
        target.proto_inputPins.push(pin);
    };
}

export function outputPin(width: number, name: string = null) {
    return function (target: Component, propertyKey: string | symbol) {
        let pin = new OutputPin("", name || propertyKey.toString(), width, null); //component不存在，会在从proto复制数据的时候设置到新的pin中
        target.proto_outputPins = target.proto_outputPins || [];
        target.proto_outputPins.push(pin);
    };
}

export interface LogicRun {
    //检查是否有问题 TODO 是不是返回个string好一点?
    validate(): boolean;

    //执行 TODO 需要参数，改个名字
    run(): void;
}

export class Wire implements LogicRun {
    fromPin: OutputPin;
    toPin: InputPin;

    setFrom(fromPin: OutputPin) {
        this.fromPin = fromPin;
    }

    setTo(toPin: InputPin) {
        this.toPin = toPin;
    }

    validate() : boolean {
        return this.fromPin && this.toPin //两端都连上了
            && this.fromPin.width === this.toPin.width; //并且宽度一致
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
    readonly component: Component;

    constructor(id: string, name: string, width: number, component: Component) {
        this.id = id;
        this.name = name;
        this.width = width;
        this.component = component;
    }

    data: number;

    writeByWire(data: number): void { //THINK 检查宽度？
        if (data < 0 || data > 1 << (this.width)) {
            //TODO 数据不合规怎么办
        }
        this.data = data;
    }

    read(): number {
        return this.data;
    }

    read1(): boolean {
        return !!this.data;
    }
}

export class OutputPin {
    readonly id: string;
    readonly name: string;
    readonly width: number;
    readonly component: Component;

    constructor(id: string, name: string, width: number, component: Component) {
        this.id = id;
        this.name = name;
        this.width = width;
        this.component = component;
    }

    data: number;

    write(data: number): void { //THINK 检查宽度？
        this.data = data;
    }

    write1(data: boolean): void {
        this.data = data ? 1 : 0;
    }
}

export class Component implements LogicRun {
    name: string;

    proto_inputPins: InputPin[];
    proto_outputPins: OutputPin[];

    inputPins: InputPin[];
    outputPins: OutputPin[];

    constructor(name: string) {
        this.name = name;
        this.initPins();
    }

    initPins() {
        // console.log("initPins");
        this.inputPins = (this.proto_inputPins || []).map(i => {
            let ip = new InputPin(`input_id_${++index}`, i.name, i.width, this);
            // @ts-ignore
            this[i.name] = ip;
            return ip;
        });
        this.outputPins = (this.proto_outputPins || []).map(i => {
            let op = new OutputPin(`output_id_${++index}`, i.name, i.width, this);
            // @ts-ignore
            this[i.name] = op;
            return op;
        });
    }

    validate(): boolean {
        return true; //TODO 其实component自己不知道pin都连到哪里去了，需要外部统计
    }

    run() {
        console.log("not implemented");
    };
}