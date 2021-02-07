import {Pin} from "../logic/Component";

export class GamePin {

    readonly isInput: boolean;
    readonly isOutput: boolean;
    readonly pin: Pin;

    constructor(pin: Pin, isInput: boolean, isOutput: boolean) {
        this.pin = pin;
        this.isInput = isInput;
        this.isOutput = isOutput;
    }
}