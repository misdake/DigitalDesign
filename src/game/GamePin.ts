import {Pin} from "../logic/Component";
import {GameWire} from "./GameWire";

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

    inWire: GameWire;
    readonly outWires: GameWire[] = [];

    setInWire(inWire: GameWire) {
        this.inWire = inWire;
    }
    addOutWire(outWire: GameWire) {
        this.outWires.push(outWire);
    }
    removeOutWire(outWire: GameWire) {
        const index = this.outWires.indexOf(outWire);
        if (index > -1) {
            this.outWires.splice(index, 1);
        }
    }
}