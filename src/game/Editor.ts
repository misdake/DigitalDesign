import {GameComp, GameCompTemplate} from "./GameComp";

export class Editor {

    constructor() {
        //TODO 输入画布div

    }

    createComponentDummy(template: GameCompTemplate) {
        //返回个啥
    }

    createComponent(template: GameCompTemplate, x: number, y: number) { //TODO dummy component?

    }

    moveComponent(gameComp: GameComp, x: number, y: number) {
        console.log("editor moveComponent", gameComp.name, x, y);
    }

    removeComponent() {

    }


    createWireDummy() {

    }

    createWire() {

    }

    removeWire() {

    }


}