import {Component} from "../logic/Component";
import {GameCompElement} from "../ui/component/GameCompElement";
import {System} from "../logic/System";

export class GameCompTemplate {
    name: string;
    type: string;
    w: number;
    h: number;
}

export class GameCompPack extends GameCompTemplate {
    x: number;
    y: number;
}

export class GameComp extends GameCompPack {
    id: number;
    component: Component;
    uiElement: GameCompElement; //to be filled by GameCompElement

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