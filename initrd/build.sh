#!/bin/sh

SUBDIRS="early-init"

cd $(dirname $0)

for SUBDIR in $SUBDIRS
do
	if ! $SUBDIR/build.sh $1
	then
		echo "$SUBDIR build failed"
		exit 1
	fi
done

# temp
cp early-init/early-init.bin initrd
