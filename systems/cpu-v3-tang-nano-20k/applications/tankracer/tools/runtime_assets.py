"""Package recorded input for the shared native level entry."""
import os

os.environ['TANKRACER_DRIVING_VERIFY'] = '0'
os.environ.setdefault('TANKRACER_DRIVING_SCOPE', 'finish-world')
os.environ.setdefault('TANKRACER_DRIVING_CASE', '0')

from driving_assets import main

if __name__ == '__main__':
    main()
