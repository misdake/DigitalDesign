import {Pin} from "../logic/Component";
import {GameWire} from "./GameWire";
import {GameComp} from "./GameComp";

export class GamePin {

    readonly gameComp: GameComp;
    readonly isInput: boolean;
    readonly isOutput: boolean;
    readonly pin: Pin;
    readonly index: number;

    constructor(gameComp: GameComp, pin: Pin, index: number, isInput: boolean, isOutput: boolean) {
        this.gameComp = gameComp;
        this.pin = pin;
        this.index = index;
        this.isInput = isInput;
        this.isOutput = isOutput;
    }

    getXy() {
        let {x, y} = this.gameComp.getXy();
        if (this.isInput) {
            return {x: x - 1, y: y + this.index};
        }
        if (this.isOutput) {
            return {x: x + this.gameComp.w, y: y + this.index};
        }
        debugger;
        return {x: 0, y: 0};
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