import {Pin} from "../logic/Component";

export class GamePin {

    readonly isInput: boolean;
    readonly isOutput: boolean;
    readonly pin: Pin;
    readonly index: number;

    constructor(pin: Pin, index: number, isInput: boolean, isOutput: boolean) {
        this.pin = pin;
        this.index = index;
        this.isInput = isInput;
        this.isOutput = isOutput;
    }
}