import {error} from "./util";

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
    fromComponent: string;
    fromPin: string;
    toComponent: string;
    toPin: string;

    static create(from: string, to: string, name?: string): WireTemplate {
        let from2 = from.split(".");
        let to2 = to.split(".");
        if (!from2 || from2.length !== 2) error("from is not X.X");
        if (!to2 || to2.length !== 2) error("to is not X.X");
        let fromComponent = from2[0] === "this" ? null : from2[0];
        let toComponent = to2[0] === "this" ? null : to2[0];
        return {
            name: name || "",
            fromComponent,
            fromPin: from2[1],
            toComponent,
            toPin: to2[1],
        };
    }
}

export class ComponentTemplate {
    type: string;
    inputPins: PinTemplate[];
    components: { name: string, type: string }[];
    outputPins: PinTemplate[];
    wires: WireTemplate[];
}