import {Component} from "./Component";

export class OutputPin {
    name: string;
    component: Component;
    width = 1;
    value: number[];

    constructor(name: string, component: Component, width: number) {
        this.name = name;
        this.component = component;
        this.width = width;
        this.value = [];
    }

    write(value: number[]) {
        this.value = value;

        if (value.length !== this.width) {
            console.log(`width not equal! expected ${this.width}, get ${value.length}`);
        }
    }
}

export class InputPin {
    name: string;
    component: Component;
    width: number;
    source: OutputPin;

    constructor(name: string, component: Component, width: number) {
        this.name = name;
        this.component = component;
        this.width = width;
    }

    connect(source: OutputPin) {
        this.source = source;

        if (source.width !== this.width) {
            console.log(`width not equal! expected ${this.width}, get ${source.width}`);
        }
    }

    readAll(): number[] {
        return this.source.value;
    }

    read(i: number): number {
        return this.source.value[i];
    }
}

export class OutputPinConstant extends OutputPin {
    constructor(name: string, width: number, constant: number[]) {
        super(name, null, width); //TODO use a constant dummy component
        this.value = constant;
    }
}

export function arrayOf(content: number, width: number) {
    let r = [];
    for (let i = 0; i < width; i++) {
        r.push(content);
    }
    return r;
}

export class OutputPin1 extends OutputPinConstant {
    constructor(width: number) {
        super("1", width, arrayOf(1, width));
    }
}

export class OutputPin0 extends OutputPinConstant {
    constructor(width: number) {
        super("0", width, arrayOf(0, width));
    }
}