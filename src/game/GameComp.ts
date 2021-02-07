import {Component} from "../logic/Component";
import {GameCompElement} from "../ui/component/GameCompElement";
import {System} from "../logic/System";
import {GamePin} from "./GamePin";

export class GameCompTemplate {
    name: string;
    type: string;
    w: number;
    h: number;

    // pinOrder? TODO
}

export class GameCompPack extends GameCompTemplate {
    x: number;
    y: number;
}

export class GameComp extends GameCompPack {
    readonly id: number;
    readonly component: Component;
    uiElement: GameCompElement; //to be filled by GameCompElement, kinda readonly

    readonly inputPins: GamePin[];
    readonly outputPins: GamePin[];

    highlight: boolean;

    constructor(id: number, system: System, pack: GameCompPack) {
        super();
        this.id = id;

        this.name = pack.name;
        this.type = pack.type;
        this.x = pack.x;
        this.y = pack.y;
        this.w = pack.w;
        this.h = pack.h;

        this.component = system.createComponent(pack.name, pack.type);

        //设置inputPins和outputPins
        this.inputPins = Object.values(this.component.inputPins).map((pin, index) => new GamePin(pin, index, true, false));
        this.outputPins = Object.values(this.component.outputPins).map((pin, index) => new GamePin(pin, index, false, true));
    }

    pack(): GameCompPack {
        return {
            name: this.name,
            type: this.type,
            x: this.x,
            y: this.y,
            w: this.w,
            h: this.h,
        };
    }
}