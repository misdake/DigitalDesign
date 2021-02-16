import {GameComp, GameCompTemplate} from "./GameComp";
import {Game} from "./Game";

export class Editor {
    private game: Game;

    constructor(game:Game) {
        //TODO 输入画布div
        this.game = game;


        // @ts-ignore
        window.deleteSelected = () => {
            if (this.selectedGameComp) {
                this.removeComponent(this.selectedGameComp);
                this.selectedGameComp = null;
            }
        };
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

    private nextCompId = 1;

    createComponent(template: GameCompTemplate, x: number, y: number): GameComp { //TODO dummy component?
        let pack = {...template, x: x, y: y};
        let comp = new GameComp(this.nextCompId++, this.game.system, pack);
        this.game.components.push(comp);
        this.doUpdate(); //TODO 支持一次添加多个
        return comp;
    }

    moveComponent(gameComp: GameComp, x: number, y: number) {
        console.log("editor moveComponent", gameComp.name, x, y);
    }

    removeComponent(gameComp: GameComp): boolean {
        const index = this.game.components.indexOf(gameComp);
        if (index > -1) {
            this.game.components.splice(index, 1);

            this.doUpdate();

            return true;
        }
        return false;
    }


    createWireDummy() {

    }

    createWire() {

    }

    removeWire() {

    }


    selectedGameComp: GameComp; //TODO move to Game?
    selectGameComp(gameComp: GameComp) {
        this.selectedGameComp = gameComp;
    }


}