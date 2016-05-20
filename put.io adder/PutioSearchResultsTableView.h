//
//  PutioSearchResultsTableView.h
//  put.io adder
//
//  Created by Nico on 03/11/14.
//  Copyright (c) 2014 Nicolas Oelgart. All rights reserved.
//

#import <Cocoa/Cocoa.h>

@interface PutioSearchResultsTableView : NSTableView <NSTableViewDataSource, NSTableViewDelegate>
{
    NSMutableArray *files;
}

@property (strong) NSMutableArray *files;

@end
