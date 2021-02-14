import {GameComp, GameCompTemplate} from "./GameComp";
import {Game} from "./Game";

export class Editor {
    private game: Game;

    constructor(game:Game) {
        //TODO 输入画布div
        this.game = game;
    }

    private callbacks: (()=>void)[] = [];

    public registerUpdate(callback: () => void) {
        this.callbacks.push(callback);
    }
    private doUpdate() {
        this.callbacks.forEach(callback => callback()); //TODO 区分不同级别的修改
    }

    createComponentDummy(template: GameCompTemplate) {
        //返回个啥
    }

    createComponent(template: GameCompTemplate, x: number, y: number) : GameComp { //TODO dummy component?
        let pack = {...template, x: x, y: y};
        let comp = new GameComp(1, this.game.system, pack); //TODO generate id
        this.game.components.push(comp);
        this.doUpdate(); //TODO 支持一次添加多个
        return comp;
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