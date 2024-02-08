cd /c/Users/circleci/project/x264/
CC=cl ./configure --enable-static --prefix=${PWD}/installed
make
make install