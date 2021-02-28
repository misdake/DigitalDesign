import {Wire} from "../logic/Component";
import {GamePin} from "./GamePin";

export class GameWireDummy {
    fromX: number;
    fromY: number;
    toX: number;
    toY: number;
}

export class GameWireDummyFrom extends GameWireDummy {
    readonly fromPin: GamePin;

    constructor(fromPin: GamePin) {
        super();
        this.fromPin = fromPin;
        //TODO 设置fromX和Y
    }

    setTo(toX: number, toY: number) {
        //TODO 设置toX和toY
    }
}
export class GameWireDummyTo extends GameWireDummy {
    readonly toPin: GamePin;

    constructor(toPin: GamePin) {
        super();
        this.toPin = toPin;
        //TODO 设置toX和Y
    }

    setFrom(fromX: number, fromY: number) {
        //TODO 设置fromX和Y
    }
}

export class GameWire {

    constructor(wire: Wire) {
        
    }


}