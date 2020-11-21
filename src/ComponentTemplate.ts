export enum PinType {
    BOOL,
    UNSIGNED,
    SIGNED,
}

export class PinTemplate {
    name: string;
    width: number;
    type: PinType;
}

export class WireTemplate {
    name: string;
    width: number;
    fromComponent: string;
    fromPin: string;
    toComponent: string;
    toPin: string;
}

export class ComponentTemplate {
    type: string;
    inputPins: PinTemplate[];
    components: { name: string, type: string }[];
    outputPins: PinTemplate[];
    wires: WireTemplate[];
}