import {Component} from "../logic/Component";
import {CompElement} from "../ui/component/CompElement";
import {System} from "../logic/System";
import {GamePin} from "./GamePin";
import {EventHost} from "../util/EventHost";
import {Events} from "../util/Events";

export class GameCompTemplate {
    name: string;
    type: string;
    w: number;
    h: number;
}

export class GameCompPack extends GameCompTemplate {
    x: number;
    y: number;

    constructor(template: GameCompTemplate, x: number, y: number) {
        super();
        this.name = template.name;
        this.type = template.type;
        this.w = template.w;
        this.h = template.h;
        this.x = x;
        this.y = y;
    }
}

export enum GameCompShowMode {
    Name = 1,
    Type,
    NameType,
}

export class GameComp extends EventHost {
    name: string;
    type: string;
    w: number;
    h: number;
    x: number;
    y: number;

    showMode: GameCompShowMode = GameCompShowMode.Type;
    movable: boolean = true;

    isTemplate: boolean = true;
    readonly id: number;
    readonly component: Component;

    private uiElement: CompElement;

    readonly inputPins: GamePin[];
    readonly outputPins: GamePin[];

    getXy() {
        return {
            x: this.x,
            y: this.y,
        }
    }

    constructor(id: number, system: System, pack: GameCompPack) {
        super();
        this.id = id;

        this.on(Events.COMPONENT_UI_CREATED, this, ui => this.uiElement = ui);

        this.name = pack.name;
        this.type = pack.type;
        this.x = pack.x;
        this.y = pack.y;
        this.w = pack.w;
        this.h = pack.h;

        this.component = system.createComponent(pack.name, pack.type);

        //设置inputPins和outputPins
        this.inputPins = Object.values(this.component.inputPins).map((pin, index) => new GamePin(this, pin, index, true, false));
        this.outputPins = Object.values(this.component.outputPins).map((pin, index) => new GamePin(this, pin, index, false, true));
    }

    updateCompValue() {
        this.inputPins.forEach(pin => pin.updatePinValue());
        this.outputPins.forEach(pin => pin.updatePinValue());
    }
}