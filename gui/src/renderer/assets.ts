// Cheese
import cheeseUrl from "../assets/sprites/cheese/cheese.png";
// Ground tiles
import tile1Url from "../assets/sprites/ground/tile1.png";
import tile2Url from "../assets/sprites/ground/tile2.png";
import tile3Url from "../assets/sprites/ground/tile3.png";
import tile4Url from "../assets/sprites/ground/tile4.png";
import tile5Url from "../assets/sprites/ground/tile5.png";
import tile6Url from "../assets/sprites/ground/tile6.png";
import tile7Url from "../assets/sprites/ground/tile7.png";
import tile8Url from "../assets/sprites/ground/tile8.png";
import tile9Url from "../assets/sprites/ground/tile9.png";
import tile10Url from "../assets/sprites/ground/tile10.png";
// Mud
import mudUrl from "../assets/sprites/mud/mud.png";
import pythonEastUrl from "../assets/sprites/players/python/east.png";
// Python
import pythonNeutralUrl from "../assets/sprites/players/python/neutral.png";
import pythonNorthUrl from "../assets/sprites/players/python/north.png";
import pythonSouthUrl from "../assets/sprites/players/python/south.png";
import pythonWestUrl from "../assets/sprites/players/python/west.png";
import ratEastUrl from "../assets/sprites/players/rat/east.png";
// Rat
import ratNeutralUrl from "../assets/sprites/players/rat/neutral.png";
import ratNorthUrl from "../assets/sprites/players/rat/north.png";
import ratSouthUrl from "../assets/sprites/players/rat/south.png";
import ratWestUrl from "../assets/sprites/players/rat/west.png";
import cornerUrl from "../assets/sprites/wall/corner.png";
// Wall
import wallUrl from "../assets/sprites/wall/wall.png";

export type PlayerSprites = {
	neutral: HTMLImageElement;
	north: HTMLImageElement;
	south: HTMLImageElement;
	east: HTMLImageElement;
	west: HTMLImageElement;
};

export type AssetMap = {
	ground: HTMLImageElement[];
	wall: HTMLImageElement;
	corner: HTMLImageElement;
	mud: HTMLImageElement;
	cheese: HTMLImageElement;
	rat: PlayerSprites;
	python: PlayerSprites;
};

function loadImage(url: string): Promise<HTMLImageElement> {
	return new Promise((resolve, reject) => {
		const img = new Image();
		img.onload = () => resolve(img);
		img.onerror = (_e) => reject(new Error(`Failed to load image: ${url}`));
		img.src = url;
	});
}

export async function loadAssets(): Promise<AssetMap> {
	const groundUrls = [
		tile1Url,
		tile2Url,
		tile3Url,
		tile4Url,
		tile5Url,
		tile6Url,
		tile7Url,
		tile8Url,
		tile9Url,
		tile10Url,
	];

	const [
		ground,
		wall,
		corner,
		mud,
		cheese,
		ratNeutral,
		ratNorth,
		ratSouth,
		ratEast,
		ratWest,
		pythonNeutral,
		pythonNorth,
		pythonSouth,
		pythonEast,
		pythonWest,
	] = await Promise.all([
		Promise.all(groundUrls.map(loadImage)),
		loadImage(wallUrl),
		loadImage(cornerUrl),
		loadImage(mudUrl),
		loadImage(cheeseUrl),
		loadImage(ratNeutralUrl),
		loadImage(ratNorthUrl),
		loadImage(ratSouthUrl),
		loadImage(ratEastUrl),
		loadImage(ratWestUrl),
		loadImage(pythonNeutralUrl),
		loadImage(pythonNorthUrl),
		loadImage(pythonSouthUrl),
		loadImage(pythonEastUrl),
		loadImage(pythonWestUrl),
	]);

	return {
		ground,
		wall,
		corner,
		mud,
		cheese,
		rat: {
			neutral: ratNeutral,
			north: ratNorth,
			south: ratSouth,
			east: ratEast,
			west: ratWest,
		},
		python: {
			neutral: pythonNeutral,
			north: pythonNorth,
			south: pythonSouth,
			east: pythonEast,
			west: pythonWest,
		},
	};
}
