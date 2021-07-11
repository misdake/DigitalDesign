import {Pin} from "../logic/Component";
import {GameComp} from "./GameComp";
import {EventHost} from "../util/EventHost";
import {Events} from "../util/Events";
import {InputPinElement, OutputPinElement} from "../ui/component/PinElement";

export class GamePin extends EventHost {

    readonly gameComp: GameComp;
    readonly isInput: boolean;
    readonly isOutput: boolean;
    readonly pin: Pin;
    readonly index: number;

    private uiElementIn: InputPinElement;
    private uiElementOut: OutputPinElement;

    constructor(gameComp: GameComp, pin: Pin, index: number, isInput: boolean, isOutput: boolean) {
        super();
        this.gameComp = gameComp;
        this.pin = pin;
        this.index = index;
        this.isInput = isInput;
        this.isOutput = isOutput;

        this.on(Events.INPUTPIN_UI_CREATED, this, ui => this.uiElementIn = ui);
        this.on(Events.OUTPUTPIN_UI_CREATED, this, ui => this.uiElementOut = ui);
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

    updatePinValue() {
        if (this.uiElementIn) this.uiElementIn.updatePinValue();
        if (this.uiElementOut) this.uiElementOut.updatePinValue();
    }
}