from importlib.metadata import Distribution


def main():
    print("tqdm version", Distribution.from_name("tqdm").version)
    from tqdm import tqdm

    for _ in tqdm(range(10)):
        pass


if __name__ == "__main__":
    main()
