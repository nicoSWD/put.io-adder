//
//  PutioSearchResultsTableView.m
//  put.io adder
//
//  Created by Nico on 03/11/14.
//  Copyright (c) 2014 Nicolas Oelgart. All rights reserved.
//

#import "PutioSearchResultsTableView.h"

@implementation PutioSearchResultsTableView

@synthesize files;


- (NSInteger)numberOfRowsInTableView:(NSTableView *)tableView
{
//    NSLog(@"%lu", (unsigned long)self.files.count);
    return 10; //self.files.count;
}


- (id)tableView:(NSTableView *)aTableView objectValueForTableColumn:(NSTableColumn *)tableColumn row:(NSInteger)row
{
    if ([tableColumn.identifier isEqualToString:@"name"])
    {
        return @"test";
//        return [[self.files objectAtIndex:row] valueForKey: @"name"];
    }
    
    return nil;
    
    NSURL *icon = [NSURL URLWithString:[[self.files objectAtIndex: row] valueForKey: @"icon"]];
    return [[NSImage alloc] initWithContentsOfURL: icon];
}


@end
